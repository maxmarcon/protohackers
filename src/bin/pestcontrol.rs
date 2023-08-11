use async_stream::{stream, try_stream};
use futures::future::BoxFuture;
use futures::Stream;
use protohackers::pestcontrol::msg::{Hello, Msg, SiteVisit};
use protohackers::pestcontrol::{Decodable, Error};
use protohackers::{pestcontrol, CliArgs, Parser, Server};
use std::collections::HashSet;
use std::io;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufWriter};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::broadcast::{Receiver, Sender};

const MAX_MSG_SIZE: usize = 4096;

fn main() {
    let args = CliArgs::parse();

    let mut connected_authorities = Arc::new(RwLock::new(HashSet::new()));

    let (sender, receiver) = tokio::sync::broadcast::channel(256);

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let connected_authorities = connected_authorities.clone();
        let receiver = sender.subscribe();
        let sender = sender.clone();
        Box::pin(async { handle_stream(tcpstream, connected_authorities, sender, receiver).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(
    mut tcpstream: TcpStream,
    connected_authorities: Arc<RwLock<HashSet<u32>>>,
    sender: Sender<SiteVisit>,
    receiver: Receiver<SiteVisit>,
) -> io::Result<()> {
    let (tcpreader, mut tcpwriter) = tcpstream.split();

    let hello_received = false;
    let reader = BufReader::new(tcpreader);
    let mut writer = BufWriter::new(tcpwriter);
    let msg = Msg::Hello(Hello::default());
    writer.write_all(&msg.encode()).await?;

    loop {}
    Ok(())
}

fn message_stream(tcpreader: ReadHalf) -> impl Stream<Item = pestcontrol::Result<Msg>> + '_ {
    let mut reader = BufReader::new(tcpreader);
    let mut buf = [0_u8; MAX_MSG_SIZE];
    // TODO: try replace try_stream! with stream! and remove the "?" from the yield statements. Does it still work?
    try_stream! {
        loop {
            reader.read_exact(&mut buf[..5]).await?;
            let length = u32::from_be_bytes(buf[1..5].try_into().unwrap()) as usize;
            if length > MAX_MSG_SIZE {
                yield Err(Error::TooLarge)?;
            }
            reader.read_exact(&mut buf[5..length]).await?;
            yield Msg::decode(&buf[..length])?;
        }
    }
}
