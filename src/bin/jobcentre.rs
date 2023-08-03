use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::jobcentre::msg;
use protohackers::jobcentre::msg::Response;
use protohackers::jobcentre::{Job, JobState, Queue};
use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::sync::broadcast::{Receiver, Sender};

fn main() {
    let args = CliArgs::parse();

    let last_client_id = Arc::new(Mutex::new(0));
    let last_job_id = Arc::new(Mutex::new(0));

    let queues = Arc::new(RwLock::new(HashMap::new()));
    let job_state = Arc::new(RwLock::new(HashMap::new()));

    let (sender, _receiver) = tokio::sync::broadcast::channel(100);

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let queues = queues.clone();
        let job_state = job_state.clone();
        let last_client_id = last_client_id.clone();
        let last_job_id = last_job_id.clone();
        let sender = sender.clone();
        let receiver = sender.subscribe();
        Box::pin(async {
            handle_stream(
                tcpstream,
                queues,
                job_state,
                last_client_id,
                last_job_id,
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
    queues: Arc<RwLock<HashMap<String, Queue>>>,
    job_state: Arc<RwLock<HashMap<u32, JobState>>>,
    last_client_id: Arc<Mutex<u32>>,
    last_job_id: Arc<Mutex<u32>>,
    sender: Sender<(u32, Job)>,
    receiver: Receiver<(u32, Job)>,
) -> io::Result<()> {
    let my_client_id = {
        let mut id = last_client_id.lock().unwrap();
        *id += 1;
        *id
    };

    let result = message_loop(
        tcpstream,
        my_client_id,
        &queues,
        &job_state,
        last_job_id,
        &sender,
        receiver,
    )
    .await;

    let working_on: Vec<_> = {
        // inefficient
        let job_state = job_state.read().unwrap();
        job_state
            .iter()
            .filter_map(|(job_id, state)| match state {
                JobState::Assigned(client_id, _) if *client_id == my_client_id => Some(*job_id),
                _ => None,
            })
            .collect()
    };
    for job_id in working_on {
        let _ = abort_job(
            my_client_id,
            job_id,
            &mut job_state.write().unwrap(),
            &mut queues.write().unwrap(),
            &sender,
        );
    }

    result
}

fn process_message(
    msg: msg::Msg,
    client_id: u32,
    last_job_id: &Arc<Mutex<u32>>,
    queues: &Arc<RwLock<HashMap<String, Queue>>>,
    job_state: &Arc<RwLock<HashMap<u32, JobState>>>,
    sender: &Sender<(u32, Job)>,
) -> Option<Response> {
    match msg {
        msg::Msg::Put(put) => {
            let mut job_id = last_job_id.lock().unwrap();
            *job_id += 1;
            let job = Job::new(*job_id, put.pri, &put.queue, put.job);
            add_job_to_queue(
                job,
                &mut queues.write().unwrap(),
                &mut job_state.write().unwrap(),
                sender,
            )
        }
        msg::Msg::Get(get) => {
            let mut queues = queues.write().unwrap();
            let mut job_state = job_state.write().unwrap();
            for queue_name in get.queues.iter() {
                queues.entry(queue_name.clone()).or_insert(Queue::default());
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
                job_state.insert(job.id, JobState::Assigned(client_id, job.clone()));
                Some(Response::ok_and_job(job))
            } else if get.wait {
                put_client_in_wait(client_id, &get.queues, &mut queues);
                None
            } else {
                Some(Response::no_job())
            }
        }
        msg::Msg::Delete(delete) => {
            let mut job_state = job_state.write().unwrap();
            match job_state.get(&delete.id) {
                None | Some(JobState::Deleted) => Some(Response::no_job()),
                _ => {
                    job_state.insert(delete.id, JobState::Deleted);
                    Some(Response::ok())
                }
            }
        }
        msg::Msg::Abort(abort) => {
            let mut job_state = job_state.write().unwrap();
            let mut queues = queues.write().unwrap();
            match abort_job(client_id, abort.id, &mut job_state, &mut queues, sender) {
                Ok(false) => Some(Response::no_job()),
                Ok(true) => Some(Response::ok()),
                Err(error) => Some(Response::error(&error)),
            }
        }
    }
}

fn add_job_to_queue(
    job: Job,
    queues: &mut HashMap<String, Queue>,
    job_state: &mut HashMap<u32, JobState>,
    sender: &Sender<(u32, Job)>,
) -> Option<Response> {
    let (response, client_id, was_waiting_in_queues) = {
        let queue = queues.entry(job.queue.clone()).or_insert(Queue::default());

        if queue.waiting_clients.is_empty() {
            job_state.insert(job.id, JobState::Unassigned);
            let job_id = job.id;
            queue.push(job);
            (Some(Response::ok_and_id(job_id)), None, None)
        } else {
            let client_id = *queue.waiting_clients.keys().next().unwrap();
            job_state.insert(job.id, JobState::Assigned(client_id, job.clone()));
            sender.send((client_id, job)).unwrap();
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

fn abort_job(
    client_id: u32,
    job_id: u32,
    job_state: &mut HashMap<u32, JobState>,
    queues: &mut HashMap<String, Queue>,
    sender: &Sender<(u32, Job)>,
) -> Result<bool, String> {
    let result = match job_state.get(&job_id) {
        Some(JobState::Assigned(worker_id, job)) if *worker_id == client_id => {
            add_job_to_queue(job.clone(), queues, job_state, sender);
            Ok(true)
        }
        Some(JobState::Assigned(_, _)) => Err(format!(
            "you are client {} and cannot abort job {} because not assigned to you",
            client_id, job_id
        )),
        _ => Ok(false),
    };
    if result == Ok(true) {
        job_state.insert(job_id, JobState::Unassigned);
    }
    result
}

fn put_client_in_wait(client_id: u32, queue_names: &[String], queues: &mut HashMap<String, Queue>) {
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
    queues: &Arc<RwLock<HashMap<String, Queue>>>,
    job_state: &Arc<RwLock<HashMap<u32, JobState>>>,
    last_job_id: Arc<Mutex<u32>>,
    sender: &Sender<(u32, Job)>,
    mut receiver: Receiver<(u32, Job)>,
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
                        if let Some(response) = process_message(msg, client_id, &last_job_id, queues, job_state, sender) {
                            tcpwriter.write_all(response.to_string().as_bytes()).await?
                        }
                    },
                    Err(error) => tcpwriter.write_all(msg::Response::error(&format!("invalid message: {:?}", error)).to_string().as_bytes()).await?
               }
            }
            job = receiver.recv() => {
                let (to_client_id, job) = job.unwrap();
                if to_client_id == client_id {
                    tcpwriter.write_all(msg::Response::ok_and_job(job).to_string().as_bytes()).await?
                }
            }
        }
    }

    Ok(())
}
