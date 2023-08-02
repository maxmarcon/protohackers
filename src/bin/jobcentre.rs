use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::jobcentre::msg;
use protohackers::jobcentre::msg::Response;
use protohackers::jobcentre::{Job, JobState, Queue};
use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, RwLockWriteGuard};
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::sync::broadcast::{Receiver, Sender};

type QueueMap = Arc<RwLock<HashMap<String, Queue>>>;

fn main() {
    let args = CliArgs::parse();

    let client_id = Arc::new(Mutex::new(0));
    let job_id = Arc::new(Mutex::new(0));

    let queues: QueueMap = Arc::new(RwLock::new(HashMap::new()));
    let job_state = Arc::new(RwLock::new(HashMap::new()));

    let (sender, _receiver) = tokio::sync::broadcast::channel(100);

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let queues = queues.clone();
        let job_state = job_state.clone();
        let client_id = client_id.clone();
        let job_id = job_id.clone();
        let sender = sender.clone();
        let receiver = sender.subscribe();
        Box::pin(async {
            handle_stream(
                tcpstream, queues, job_state, client_id, job_id, sender, receiver,
            )
            .await
        })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

fn message_stream(mut tcpreader: ReadHalf) -> impl Stream<Item = msg::Result<msg::Msg>> + '_ {
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
    job_state: Arc<RwLock<HashMap<u32, JobState>>>,
    client_id: Arc<Mutex<u32>>,
    job_id: Arc<Mutex<u32>>,
    sender: Sender<Job>,
    receiver: Receiver<Job>,
) -> io::Result<()> {
    let my_client_id = {
        let mut id = client_id.lock().unwrap();
        *id += 1;
        *id
    };

    let result = message_loop(
        tcpstream,
        my_client_id,
        queues,
        &job_state,
        job_id,
        sender,
        receiver,
    )
    .await;

    let working_on = {
        let job_state = job_state.read().unwrap();
        job_state
            .iter()
            .find(|(_, &state)| state == JobState::Assigned(my_client_id))
            .map(|(job_id, _)| *job_id)
    };
    if let Some(job_id) = working_on {
        job_state
            .write()
            .unwrap()
            .insert(job_id, JobState::Unassigned);
    }

    result
}

fn process_message(
    msg: msg::Msg,
    client_id: u32,
    job_id: &Arc<Mutex<u32>>,
    queues: &QueueMap,
    job_state: &Arc<RwLock<HashMap<u32, JobState>>>,
    sender: &Sender<Job>,
) -> Option<Response> {
    match msg {
        msg::Msg::Put(put) => {
            let mut queues = queues.write().unwrap();
            let (response, client_id, was_waiting_in_queues) = {
                let queue = queues.entry(put.queue.clone()).or_insert(Queue::default());

                let mut job_id = job_id.lock().unwrap();
                *job_id += 1;
                let job = Job::new(*job_id, put.pri, &put.queue, put.job);
                if queue.waiting_clients.is_empty() {
                    job_state
                        .write()
                        .unwrap()
                        .insert(job.id, JobState::Unassigned);
                    queue.push(job);
                    (Some(Response::ok_and_id(*job_id)), None, None)
                } else {
                    let client_id = *queue.waiting_clients.keys().next().unwrap();
                    job_state
                        .write()
                        .unwrap()
                        .insert(job.id, JobState::Assigned(client_id));
                    sender.send(job).unwrap();
                    (
                        None,
                        Some(client_id),
                        Some(queue.waiting_clients.remove(&client_id).unwrap()),
                    )
                }
            };
            if let Some(client_id) = client_id {
                for queue_name in was_waiting_in_queues.unwrap() {
                    queues
                        .get_mut(&queue_name)
                        .unwrap()
                        .waiting_clients
                        .remove(&client_id);
                }
            }
            response
        }
        msg::Msg::Get(get) => {
            let mut queues = queues.write().unwrap();
            let mut job_state = job_state.write().unwrap();
            for queue_name in get.queues.iter() {
                if !queues.contains_key(queue_name) {
                    return Some(Response::error(&format!(
                        "queue {} does nto exist",
                        queue_name
                    )));
                }
            }
            if let Some(queue_with_max_prio_job) = get
                .queues
                .iter()
                .flat_map(|queue_name| queues.get_mut(queue_name).unwrap().peek(&job_state))
                .max_by_key(|j| j.prio)
                .map(|j| j.queue)
            {
                let job = queues
                    .get_mut(&queue_with_max_prio_job)
                    .unwrap()
                    .pop(&job_state)
                    .unwrap();
                job_state.insert(job.id, JobState::Assigned(client_id));
                Some(Response::ok_and_id(job.id))
            } else if get.wait {
                put_client_in_wait(client_id, &get.queues, queues);
                None
            } else {
                Some(Response::no_job())
            }
        }
        msg::Msg::Delete(delete) => {
            let mut job_state = job_state.write().unwrap();
            match job_state.get(&delete.id) {
                None => Some(Response::no_job()),
                Some(JobState::Deleted) => Some(Response::no_job()),
                _ => {
                    job_state.insert(delete.id, JobState::Deleted);
                    Some(Response::ok())
                }
            }
        }
        msg::Msg::Abort(abort) => {
            let mut job_state = job_state.write().unwrap();
            match job_state.get(&abort.id) {
                Some(JobState::Assigned(worker_id)) if *worker_id == client_id => {
                    job_state.insert(abort.id, JobState::Unassigned);
                    Some(Response::ok())
                }
                Some(JobState::Assigned(_)) => {
                    Some(Response::error("you are not working on this job!"))
                }
                _ => Some(Response::no_job()),
            }
        }
    }
}

fn put_client_in_wait(
    client_id: u32,
    queue_names: &[String],
    mut queues: RwLockWriteGuard<HashMap<String, Queue>>,
) {
    for queue_name in queue_names {
        queues
            .get_mut(queue_name)
            .unwrap()
            .waiting_clients
            .insert(client_id, queue_names.to_owned());
    }
}

async fn message_loop(
    mut tcpstream: TcpStream,
    client_id: u32,
    queues: QueueMap,
    job_state: &Arc<RwLock<HashMap<u32, JobState>>>,
    job_id: Arc<Mutex<u32>>,
    sender: Sender<Job>,
    mut receiver: Receiver<Job>,
) -> io::Result<()> {
    let (tcpreader, mut tcpwriter) = tcpstream.split();

    let incoming_messages = message_stream(tcpreader);
    pin_mut!(incoming_messages);
    loop {
        tokio::select! {
            message = incoming_messages.next() => {
               if message.is_none() {
                    break;
               }
               match message.unwrap() {
                    Ok(msg) =>  {
                        if let Some(response) = process_message(msg, client_id, &job_id, &queues, job_state, &sender) {
                            tcpwriter.write_all(response.to_string().as_bytes()).await?
                        }
                    },
                    Err(error) => tcpwriter.write_all(msg::Response::error(&format!("invalid message: {:?}", error)).to_string().as_bytes()).await?
               }
            }
            job = receiver.recv() => {
                let job = job.unwrap();
                if job_state.read().unwrap().get(&job.id) == Some(&JobState::Assigned(client_id)) {
                    tcpwriter.write_all(msg::Response::ok_and_job(job).to_string().as_bytes()).await?
                }
            }
        }
    }

    Ok(())
}
