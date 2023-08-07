use futures::future::BoxFuture;
use md5::{Context, Digest};
use protohackers::vcs;
use protohackers::vcs::command;
use protohackers::vcs::command::Command;
use protohackers::{CliArgs, Parser, Server};
use std::cmp::min;
use std::env::temp_dir;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::{fs, io};
use uuid::Uuid;

const MAX_FILE_SIZE: usize = 10_000_000;

static HELP_MSG: &str = "OK usage: HELP|GET|PUT|LIST";

fn main() {
    let args = CliArgs::parse();
    vcs::create_working_dir().unwrap();

    let root_dir = Arc::new(RwLock::new(vcs::Dir::default()));

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let root_dir = root_dir.clone();
        Box::pin(async { handle_stream(tcpstream, root_dir).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(
    mut tcpstream: TcpStream,
    root_dir: Arc<RwLock<vcs::Dir>>,
) -> io::Result<()> {
    let (tcpreader, mut tcpwriter) = tcpstream.split();
    let mut reader = BufReader::new(tcpreader);

    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    loop {
        tcpwriter.write_all(b"READY\n").await?;
        let read = reader.read_until(b'\n', &mut buf).await?;
        if read == 0 {
            break;
        }
        match command::parse(&buf[..read - 1]) {
            Ok(Command::Help) => {
                tcpwriter
                    .write_all(format!("{}\n", HELP_MSG).as_bytes())
                    .await?;
            }
            Ok(Command::List(list)) => {
                let response = match root_dir.read().unwrap().find_dir(&list.dir) {
                    Some(dir) => format!("{}\n", dir),
                    None => "OK 0\n".to_owned(),
                };
                tcpwriter.write_all(response.as_bytes()).await?;
            }
            Ok(Command::Get(get)) => {
                let file_path = root_dir
                    .read()
                    .unwrap()
                    .find_file(&get.filename)
                    .map(|file| file.file_path(get.revision));
                let (response, file) = match file_path {
                    Some(path) => {
                        let file = fs::File::open(path).await?;
                        (
                            format!("OK {}\n", file.metadata().await.unwrap().len()),
                            Some(file),
                        )
                    }
                    None => ("ERR no such file\n".to_string(), None),
                };
                tcpwriter.write_all(response.as_bytes()).await?;
                if let Some(file) = file {
                    send_file(&mut tcpwriter, file).await?
                }
            }
            Ok(Command::Put(command::Put { length, .. })) if length > MAX_FILE_SIZE => {
                tcpwriter.write_all(b"ERR file too long\n").await?;
            }
            Ok(Command::Put(put)) => {
                let (temp_file_path, digest) = save_file(&mut reader, put.length).await?;
                let (current_revision, new_revision) = root_dir
                    .write()
                    .unwrap()
                    .add_revision(&put.filename, digest)?;
                if new_revision {
                    let dest_file_path =
                        PathBuf::from(vcs::WORKING_DIR).join(format!("{:x}", digest));
                    fs::rename(temp_file_path, dest_file_path).await?;
                } else {
                    fs::remove_file(temp_file_path).await?;
                }
                tcpwriter
                    .write_all(format!("OK r{}\n", current_revision).as_bytes())
                    .await?
            }
            Err(error) => {
                tcpwriter
                    .write_all(format!("{}\n", error).as_bytes())
                    .await?;
                if let command::Error::IllegalMethod(_) = error {
                    break;
                }
            }
        }
        buf.drain(..read);
    }

    Ok(())
}

async fn save_file(
    reader: &mut BufReader<ReadHalf<'_>>,
    file_len: usize,
) -> io::Result<(PathBuf, Digest)> {
    let mut buf = [0; 4096];
    let mut total_read: usize = 0;
    let mut context = Context::new();
    let temp_file_path = temp_dir().join(Uuid::new_v4().to_string());
    let mut temp_file = fs::File::create(&temp_file_path).await?;
    while total_read < file_len {
        let to_read = min(4096, file_len - total_read);
        let read = reader.read_exact(&mut buf[..to_read]).await?;
        if read == 0 {
            return Err(io::Error::from(ErrorKind::UnexpectedEof));
        }
        context.consume(&buf[..read]);
        temp_file.write_all(&buf[..read]).await?;
        total_read += read;
    }
    let digest = context.compute();
    Ok((temp_file_path, digest))
}

async fn send_file(tcpwriter: &mut WriteHalf<'_>, mut file: fs::File) -> io::Result<()> {
    let mut buf = [0; 4096];
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        tcpwriter.write_all(&buf[..read]).await?;
    }
    Ok(())
}
