use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::jobcentre::msg;
use protohackers::jobcentre::{Job, Queue};
use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;

type QueueMap = Arc<HashMap<String, Arc<RwLock<Queue>>>>;
type JobMap = Arc<HashMap<u32, Arc<RwLock<Job>>>>;

fn main() {
    let args = CliArgs::parse();

    let client_id = Arc::new(Mutex::new(0));
    let job_id = Arc::new(Mutex::new(0));

    let queues: QueueMap = Arc::new(HashMap::new());
    let jobs: JobMap = Arc::new(HashMap::new());

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let queues = queues.clone();
        let jobs = jobs.clone();
        let client_id = client_id.clone();
        let job_id = job_id.clone();
        Box::pin(async { handle_stream(tcpstream, queues, jobs, client_id, job_id).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

fn message_stream(mut tcpreader: OwnedReadHalf) -> impl Stream<Item = msg::Result<msg::Msg>> {
    let mut buf: Vec<u8> = Vec::new();
    try_stream! {
        loop {
            while let Some(eom) = buf.iter().enumerate().find(|(_, b)| **b == b'\n').map(|(idx, _)| idx) {
                yield msg::parse(&String::from_utf8(buf.drain(..eom).collect())?)?;
                buf.drain(..1);
            }
            let read = tcpreader.read_buf(&mut buf).await?;
            if read == 0 {
                break;
            }
        }
    }
}

async fn handle_stream(
    tcpstream: TcpStream,
    queues: QueueMap,
    jobs: JobMap,
    client_id: Arc<Mutex<u32>>,
    job_id: Arc<Mutex<u32>>,
) -> io::Result<()> {
    let (tcpreader, mut tcpwriter) = tcpstream.into_split();

    let my_client_id = {
        let mut id = client_id.lock().unwrap();
        *id += 1;
        *id
    };

    let incoming_messages = message_stream(tcpreader);
    pin_mut!(incoming_messages);
    loop {
        tokio::select! {
            message = incoming_messages.next() => {
               if message.is_none() {
                    break;
               }
               match message.unwrap() {
                    Ok(_) => {},
                    Err(error) => tcpwriter.write_all(msg::Response::error("invalid message").to_string().as_bytes()).await?
               }
            }
        }
    }

    Ok(())
}
