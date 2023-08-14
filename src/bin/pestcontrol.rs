use async_stream::stream;
use futures::future::BoxFuture;
use futures::Stream;
use futures::StreamExt;
use protohackers::pestcontrol::msg::{
    CreatePolicy, DialAuth, ErrorMsg, Hello, Msg, Policy, SiteVisit,
};
use protohackers::pestcontrol::{Action, Decodable, Error, Population};
use protohackers::{pestcontrol, CliArgs, Parser, Server};
use std::collections::hash_map::Entry::Occupied;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::{Arc, RwLock};
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::pin;
use tokio::sync::broadcast::{Receiver, Sender};

type SitePolicy = HashMap<u32, HashMap<String, (Option<u32>, Action)>>;

type SiteVisitMap = HashMap<String, u32>;

const MAX_MSG_SIZE: usize = 65536;

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
    sender: Sender<(u32, SiteVisitMap)>,
) -> io::Result<()> {
    let (tcpreader, mut tcpwriter) = tcpstream.split();

    let mut hello_received = false;
    let msg = Msg::Hello(Hello::default());
    tcpwriter.write_all(&msg.encode()).await?;

    let message_stream = message_stream(tcpreader);
    pin!(message_stream);

    while let Some(msg) = message_stream.next().await {
        match msg {
            Err(error) => {
                match &error {
                    Error::IO(_) => {}
                    error => {
                        tcpwriter
                            .write_all(&Msg::Error(ErrorMsg::from(error)).encode())
                            .await?
                    }
                }
                return Err(error.into());
            }
            Ok(msg) => match msg {
                Msg::Hello(_) if !hello_received => {
                    hello_received = true;
                }
                Msg::SiteVisit(site_visit) if hello_received => {
                    let site_visit_map = match make_site_visit_map(&site_visit) {
                        Ok(site_visit_map) => site_visit_map,
                        Err(error) => {
                            tcpwriter
                                .write_all(&Msg::Error(ErrorMsg::from(&error)).encode())
                                .await?;
                            return Err(error.into());
                        }
                    };
                    let mut connected_authorities_locked = connected_authorities.write().unwrap();
                    if !connected_authorities_locked.contains(&site_visit.site) {
                        connected_authorities_locked.insert(site_visit.site);

                        let connected_authorities = connected_authorities.clone();
                        let site_policy = site_policy.clone();
                        let receiver = sender.subscribe();
                        tokio::spawn(async move {
                            println!("starting authority client for site {}", site_visit.site);
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
                    sender.send((site_visit.site, site_visit_map)).unwrap();
                }
                _ => {
                    tcpwriter
                        .write_all(&Msg::Error(ErrorMsg::from(&Error::Unexpected)).encode())
                        .await?;
                    break;
                }
            },
        }
    }
    Ok(())
}

fn make_site_visit_map(site_visit: &SiteVisit) -> pestcontrol::Result<SiteVisitMap> {
    let mut site_visit_map = HashMap::new();
    for Population { species, count } in site_visit.populations.iter() {
        if let Some(old_count) = site_visit_map.insert(species.to_string(), *count) {
            if old_count != *count {
                return Err(Error::ConflictingCount);
            }
        }
    }
    Ok(site_visit_map)
}

#[derive(PartialEq, Debug)]
enum Expected {
    Hello,
    TargetPopulations,
    PolicyResult(String),
    Ok,
}

async fn authority_client(
    site: u32,
    site_policy: Arc<RwLock<SitePolicy>>,
    mut receiver: Receiver<(u32, SiteVisitMap)>,
) -> io::Result<()> {
    let mut tcpstream = TcpStream::connect(AUTHORITY_SERVER).await?;
    let (tcpreader, mut tcpwriter) = tcpstream.split();
    let message_stream = message_stream(tcpreader);
    pin!(message_stream);

    tcpwriter
        .write_all(&Msg::Hello(Hello::default()).encode())
        .await?;

    let mut expected_messages = VecDeque::from(vec![Expected::Hello]);
    let mut target_populations = HashMap::new();
    loop {
        let expected_next = expected_messages.pop_front();
        tokio::select! {
            Some(msg) = message_stream.next() => {
                if expected_next.is_none() {
                    tcpwriter.write_all(&Msg::Error(ErrorMsg::from(&Error::Unexpected)).encode()).await?;
                    break;
                }
                let msg = msg?;
                let expected_next= expected_next.unwrap();
                match msg {
                    Msg::Hello(_) if expected_next == Expected::Hello => {
                        tcpwriter.write_all(&Msg::DialAuth(DialAuth{site}).encode()).await?;
                        expected_messages.push_back(Expected::TargetPopulations);
                    },
                    Msg::TargetPopulations(target_populations_msg) if expected_next == Expected::TargetPopulations && target_populations_msg.site == site => {
                        for population in target_populations_msg.populations {
                            target_populations.insert(population.species, (population.min, population.max));
                        }
                    },
                    Msg::Ok if expected_next == Expected::Ok => {},
                    Msg::PolicyResult(policy_result) if matches!(expected_next, Expected::PolicyResult(_)) => {
                        if let Expected::PolicyResult(species) = expected_next {
                            let mut site_policy = site_policy.write().unwrap();
                            site_policy.entry(site).or_default().entry(species).and_modify(|policy| {
                                policy.0 = Some(policy_result.policy)
                            });
                        }
                    },
                    _ => {
                        tcpwriter.write_all(&Msg::Error(ErrorMsg::from(&Error::Unexpected)).encode()).await?;
                        break;
                    }
                }
            },
            Ok((site_visit_site, site_visit_map)) = receiver.recv(), if expected_next.is_none() => {
                if site_visit_site == site {
                    let messages = process_site_visit(site, site_visit_map, &site_policy,  &target_populations);
                    for message in messages {
                        tcpwriter.write_all(&message.encode()).await?;
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
    observed_populations: SiteVisitMap,
    site_policy: &Arc<RwLock<SitePolicy>>,
    target_populations: &HashMap<String, (u32, u32)>,
) -> Vec<Msg> {
    let mut site_policy = site_policy.write().unwrap();
    let site_policies = site_policy.entry(site).or_default();
    let mut messages = Vec::new();
    for (species, (min, max)) in target_populations {
        let count = observed_populations.get(species).unwrap_or(&0);
        if count < min {
            messages.append(&mut maybe_create_policy(
                site_policies,
                species.to_string(),
                Action::Conserve,
            ));
        } else if count > max {
            messages.append(&mut maybe_create_policy(
                site_policies,
                species.to_string(),
                Action::Cull,
            ));
        } else if let Some(msg) = maybe_delete_policy(site_policies, species.to_string()) {
            messages.push(msg);
        }
    }
    messages
}

fn maybe_create_policy(
    site_policies: &mut HashMap<String, (Option<u32>, Action)>,
    species: String,
    action: Action,
) -> Vec<Msg> {
    let mut messages = Vec::new();
    let create_policy = {
        if let Occupied(entry) = site_policies.entry(species.clone()) {
            if entry.get().1 != action {
                if entry.get().0.is_none() {
                    panic!("trying to delete policy without an id for {}", species);
                }
                messages.push(Msg::DeletePolicy(Policy {
                    policy: entry.get().0.unwrap(),
                }));
                entry.remove();
                true
            } else {
                false
            }
        } else {
            true
        }
    };
    if create_policy {
        site_policies.insert(species.clone(), (None, action));
        messages.push(Msg::CreatePolicy(CreatePolicy { species, action }));
    }
    messages
}

fn maybe_delete_policy(
    site_policies: &mut HashMap<String, (Option<u32>, Action)>,
    species: String,
) -> Option<Msg> {
    if let Some((policy_id, _action)) = site_policies.remove(&species) {
        if policy_id.is_none() {
            panic!("trying to delete policy without an id for {}", species);
        }
        Some(Msg::DeletePolicy(Policy {
            policy: policy_id.unwrap(),
        }))
    } else {
        None
    }
}
