use async_stream::stream;
use futures::future::BoxFuture;
use futures::Stream;
use futures::StreamExt;
use protohackers::pestcontrol::msg::{ErrorMsg, Hello, Msg, SiteVisit};
use protohackers::pestcontrol::{Action, Decodable, Error};
use protohackers::{pestcontrol, CliArgs, Parser, Server};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufWriter};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::pin;
use tokio::sync::broadcast::{Receiver, Sender};

type SitePolicy = HashMap<u32, HashMap<String, (u32, Action)>>;

const MAX_MSG_SIZE: usize = 4096;

static AUTHORITY_SERVER: &str = "pestcontrol.protohackers.com:20547";

fn main() {
    let args = CliArgs::parse();

    let connected_authorities = Arc::new(RwLock::new(HashSet::new()));
    let site_policy = Arc::new(RwLock::new(HashMap::new()));

    let (sender, receiver) = tokio::sync::broadcast::channel(256);

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let connected_authorities = connected_authorities.clone();
        let site_policy = site_policy.clone();
        let receiver = sender.subscribe();
        let sender = sender.clone();
        Box::pin(async {
            handle_stream(
                tcpstream,
                connected_authorities,
                site_policy,
                sender,
                receiver,
            )
            .await
        })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(
    mut tcpstream: TcpStream,
    connected_authorities: Arc<RwLock<HashSet<u32>>>,
    site_policy: Arc<RwLock<SitePolicy>>,
    sender: Sender<SiteVisit>,
    receiver: Receiver<SiteVisit>,
) -> io::Result<()> {
    let (tcpreader, tcpwriter) = tcpstream.split();

    let mut hello_received = false;
    let mut writer = BufWriter::new(tcpwriter);
    let msg = Msg::Hello(Hello::default());
    writer.write_all(&msg.encode()).await?;

    let message_stream = message_stream(tcpreader);
    pin!(message_stream);

    loop {
        while let Some(msg) = message_stream.next().await {
            match msg {
                Err(error) => {
                    match &error {
                        Error::IO(_) => {}
                        error => writer.write_all(&ErrorMsg::from(error).encode()).await?,
                    }
                    return Err(error.into());
                }
                Ok(msg) => match msg {
                    Msg::Hello(_) if !hello_received => {
                        hello_received = true;
                    }
                    Msg::SiteVisit(site_visit) if hello_received => {
                        let mut connected_authorities_write =
                            connected_authorities.write().unwrap();
                        if !connected_authorities_write.contains(&site_visit.site) {
                            let connected_authorities = connected_authorities.clone();
                            let site_policy = site_policy.clone();
                            let receiver = sender.subscribe();
                            tokio::spawn(async move {
                                let result = authority_client(
                                    site_visit.site,
                                    connected_authorities.clone(),
                                    site_policy,
                                    receiver,
                                )
                                .await;
                                connected_authorities
                                    .write()
                                    .unwrap()
                                    .remove(&site_visit.site);
                                result
                            });
                            connected_authorities_write.insert(site_visit.site);
                        }
                        sender.send(site_visit).unwrap();
                    }
                    _ => {
                        writer
                            .write_all(&Msg::Error(ErrorMsg::new("unexpected messsage")).encode())
                            .await?;
                        break;
                    }
                },
            }
        }
    }
}

enum Expected {
    Hello,
    TargetPopulations,
    PolicyResult(String),
    Ok,
}

async fn authority_client(
    site: u32,
    connected_authorities: Arc<RwLock<HashSet<u32>>>,
    connected_site_policy: Arc<RwLock<SitePolicy>>,
    mut receiver: Receiver<SiteVisit>,
) -> io::Result<()> {
    let mut tcpstream = TcpStream::connect(AUTHORITY_SERVER).await?;
    let (tcpreader, tcpwriter) = tcpstream.split();
    let mut writer = BufWriter::new(tcpwriter);
    let message_stream = message_stream(tcpreader);
    pin!(message_stream);

    writer
        .write_all(&Msg::Hello(Hello::default()).encode())
        .await?;

    let next_expected = vec![Expected::Hello, Expected::TargetPopulations];

    loop {
        tokio::select! {
            msg = message_stream.next() => {

            },
            site_visit = receiver.recv() => {

            }
        }
    }
}

fn message_stream(tcpreader: ReadHalf) -> impl Stream<Item = pestcontrol::Result<Msg>> + '_ {
    let mut reader = BufReader::new(tcpreader);
    let mut buf = [0_u8; MAX_MSG_SIZE];
    stream! {
        loop {
            reader.read_exact(&mut buf[..5]).await?;
            let length = u32::from_be_bytes(buf[1..5].try_into().unwrap()) as usize;
            if length > MAX_MSG_SIZE {
                yield Err(Error::TooLarge);
            }
            reader.read_exact(&mut buf[5..length]).await?;
            yield Msg::decode(&buf[..length]);
        }
    }
}
