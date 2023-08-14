use async_stream::stream;
use futures::future::BoxFuture;
use futures::Stream;
use futures::StreamExt;
use protohackers::pestcontrol::msg::{CreatePolicy, ErrorMsg, Hello, Msg, Policy, SiteVisit};
use protohackers::pestcontrol::{Action, Decodable, Error, Population};
use protohackers::{pestcontrol, CliArgs, Parser, Server};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::io::{AsyncWriteExt, BufWriter};
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

    let (sender, _receiver) = tokio::sync::broadcast::channel(256);

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let connected_authorities = connected_authorities.clone();
        let site_policy = site_policy.clone();
        let sender = sender.clone();
        Box::pin(async {
            handle_stream(tcpstream, connected_authorities, site_policy, sender).await
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
                        if !connected_authorities
                            .read()
                            .unwrap()
                            .contains(&site_visit.site)
                        {
                            let connected_authorities = connected_authorities.clone();
                            let site_policy = site_policy.clone();
                            let receiver = sender.subscribe();
                            tokio::spawn(async move {
                                println!("starting authority client for site {}", site_visit.site);
                                connected_authorities
                                    .write()
                                    .unwrap()
                                    .insert(site_visit.site);
                                let result =
                                    authority_client(site_visit.site, site_policy, receiver).await;
                                connected_authorities
                                    .write()
                                    .unwrap()
                                    .remove(&site_visit.site);
                                print!("authority client for site {} terminated", site_visit.site);
                                if let Err(error) = &result {
                                    print!(" with error: {}", error)
                                }
                                println!();
                                result
                            });
                        }
                        sender.send(site_visit).unwrap();
                    }
                    _ => {
                        writer
                            .write_all(&Msg::Error(ErrorMsg::from(&Error::Unexpected)).encode())
                            .await?;
                        break;
                    }
                },
            }
        }
    }
}

#[derive(PartialEq)]
enum Expected {
    Hello,
    TargetPopulations,
    PolicyResult(String),
    Ok,
}

async fn authority_client(
    site: u32,
    site_policy: Arc<RwLock<SitePolicy>>,
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

    let mut expected_messages = VecDeque::from(vec![Expected::Hello, Expected::TargetPopulations]);
    let mut target_populations = HashMap::new();
    loop {
        let expected_next = expected_messages.pop_front();
        tokio::select! {
            Some(msg) = message_stream.next() => {
                if expected_next.is_none() {
                    writer.write_all(&Msg::Error(ErrorMsg::from(&Error::Unexpected)).encode()).await?;
                    break;
                }
                let msg = msg?;
                let expected= expected_next.unwrap();
                match msg {
                    Msg::Hello(_) if expected == Expected::Hello => {},
                    Msg::TargetPopulations(target_populations_msg) if expected == Expected::TargetPopulations && target_populations_msg.site == site => {
                        for population in target_populations_msg.populations {
                            target_populations.insert(population.species, (population.min, population.max));
                        }
                    },
                    Msg::Ok if expected == Expected::Ok => {},
                    Msg::PolicyResult(policy_result) if matches!(expected, Expected::PolicyResult(_)) => {
                        if let Expected::PolicyResult(species) = expected {
                            let mut site_policy = site_policy.write().unwrap();
                            site_policy.entry(site).or_default().entry(species).and_modify(|policy| {
                                policy.0 = policy_result.policy
                            });
                        }
                    },
                    _ => {
                        writer.write_all(&Msg::Error(ErrorMsg::from(&Error::Unexpected)).encode()).await?;
                        break;
                    }
                }
            },
            Ok(site_visit) = receiver.recv(), if expected_next.is_none() => {
                if site_visit.site == site {
                    let messages = process_site_visit(site, site_visit.populations, &site_policy,  &target_populations);
                    for message in messages {
                        writer.write_all(&message.encode()).await?;
                        match message {
                            Msg::DeletePolicy(_) => expected_messages.push_back(Expected::Ok),
                            Msg::CreatePolicy(CreatePolicy{species, ..}) => expected_messages.push_back(Expected::PolicyResult(species)),
                            _ => panic!("unexpected message sent to server")
                        }
                    }
                }
            },
            else => break
        }
    }
    Ok(())
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

fn process_site_visit(
    site: u32,
    observed_populations: Vec<Population>,
    site_policy: &Arc<RwLock<SitePolicy>>,
    target_populations: &HashMap<String, (u32, u32)>,
) -> Vec<Msg> {
    let mut site_policy = site_policy.write().unwrap();
    let site_policies = site_policy.entry(site).or_default();
    let mut messages = Vec::new();
    for population in observed_populations {
        if let Some(&(min, max)) = target_populations.get(&population.species) {
            if population.count < min {
                messages.append(&mut maybe_create_policy(
                    site_policies,
                    population.species,
                    Action::Conserve,
                ));
            } else if population.count > max {
                messages.append(&mut maybe_create_policy(
                    site_policies,
                    population.species,
                    Action::Cull,
                ));
            } else if let Some(msg) = maybe_delete_policy(site_policies, population.species) {
                messages.push(msg);
            }
        }
    }

    Vec::new()
}

fn maybe_create_policy(
    site_policies: &mut HashMap<String, (u32, Action)>,
    species: String,
    action: Action,
) -> Vec<Msg> {
    let mut messages = Vec::new();
    let create_policy = {
        if let Some((policy_id, current_action)) = site_policies.get(&species) {
            if *current_action != action {
                messages.push(Msg::DeletePolicy(Policy { policy: *policy_id }));
                true
            } else {
                false
            }
        } else {
            true
        }
    };
    if create_policy {
        messages.push(Msg::CreatePolicy(CreatePolicy { species, action }));
    }
    messages
}

fn maybe_delete_policy(
    site_policies: &mut HashMap<String, (u32, Action)>,
    species: String,
) -> Option<Msg> {
    if let Some((policy_id, _)) = site_policies.remove(&species) {
        Some(Msg::DeletePolicy(Policy { policy: policy_id }))
    } else {
        None
    }
}
