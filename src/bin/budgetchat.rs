use crate::Error::Disconnected;
use futures::future::BoxFuture;
use protohackers::budgetchat::parse_message;
use protohackers::{CliArgs, Parser, Server};
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::sync::broadcast::Sender;
use tokio::sync::{broadcast, Mutex};

fn main() {
    let args = CliArgs::parse();

    let (sender, _receiver) = broadcast::channel(100);

    let users = Arc::new(Mutex::new(HashSet::new()));

    let handler: Arc<_> = {
        Arc::new(move |tcp_stream| -> BoxFuture<'static, io::Result<()>> {
            let sender = sender.clone();
            let users = users.clone();
            Box::pin(async {
                handle_stream(tcp_stream, sender, users)
                    .await
                    .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))
            })
        })
    };

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

#[derive(Clone, Debug)]
enum ChatMsgSender {
    System,
    User,
}

#[derive(Clone, Debug)]
struct ChatMsg {
    pub subject: String,
    sender: ChatMsgSender,
    msg: String,
}

impl ChatMsg {
    pub fn from_user(subject: &str, msg: String) -> Self {
        Self {
            subject: subject.to_owned(),
            sender: ChatMsgSender::User,
            msg,
        }
    }
    pub fn from_system(subject: &str, msg: String) -> Self {
        Self {
            subject: subject.to_owned(),
            sender: ChatMsgSender::System,
            msg,
        }
    }
}

impl Display for ChatMsg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.sender {
            ChatMsgSender::User => writeln!(f, "[{}] {}", self.subject, self.msg),
            ChatMsgSender::System => writeln!(f, "* {}", self.msg),
        }
    }
}

enum Error {
    InvalidName,
    Disconnected,
    IO(io::Error),
    Receiver(RecvError),
    Sender(SendError<ChatMsg>),
}

type Result<T> = std::result::Result<T, Error>;

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::IO(value)
    }
}

impl From<RecvError> for Error {
    fn from(value: RecvError) -> Self {
        Error::Receiver(value)
    }
}

impl From<SendError<ChatMsg>> for Error {
    fn from(value: SendError<ChatMsg>) -> Self {
        Error::Sender(value)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidName => f.write_str("InvalidName"),
            Disconnected => f.write_str("Disconnected"),
            Error::IO(io_error) => std::fmt::Display::fmt(io_error, f),
            Error::Receiver(recv_error) => std::fmt::Display::fmt(recv_error, f),
            Error::Sender(send_error) => std::fmt::Display::fmt(send_error, f),
        }
    }
}

async fn handle_stream(
    mut tcp_stream: TcpStream,
    sender: Sender<ChatMsg>,
    users: Arc<Mutex<HashSet<String>>>,
) -> Result<()> {
    let mut buffer = Vec::new();
    let mut user_name = None;
    let mut receiver = sender.subscribe();
    tcp_stream.write_all(b"Hello, what's your name?\n").await?;

    loop {
        tokio::select! {
            result = recv_and_process(&mut tcp_stream, &mut buffer, &mut user_name, &sender, &users) => {
                if let Err(error) = result {
                    if user_name.is_some() {
                        sender.send(ChatMsg::from_system(user_name.as_ref().unwrap(), format!("{} has left the room", user_name.as_ref().unwrap())))?;
                        users.lock().await.remove(user_name.as_ref().unwrap());
                    }
                    match error {
                        Disconnected => return Ok(()),
                        _ => return Err(error)
                    }
                }
            }
            result = receiver.recv() => {
                let chat_msg = result?;
                if user_name.is_some() && !chat_msg.subject.eq(user_name.as_ref().unwrap()) {
                    tcp_stream.write_all(chat_msg.to_string().as_bytes()).await?;
                }
            }
        }
    }
}

async fn recv_and_process(
    tcp_stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    user_name: &mut Option<String>,
    sender: &Sender<ChatMsg>,
    users: &Arc<Mutex<HashSet<String>>>,
) -> Result<()> {
    let read_bytes = tcp_stream.read_buf(buffer).await?;
    if read_bytes == 0 {
        return Err(Disconnected);
    }
    while let Some(msg) = parse_message(buffer)? {
        if user_name.is_none() {
            *user_name = match validate_name(msg) {
                Ok(user_name) => Some(user_name),
                Err(error) => {
                    tcp_stream.write_all(error.to_string().as_bytes()).await?;
                    return Err(error);
                }
            };
            let user_name: &str = user_name.as_ref().unwrap();
            sender
                .send(ChatMsg::from_system(
                    user_name,
                    format!("{} has entered the room", user_name),
                ))
                .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
            tcp_stream
                .write_all(
                    format!(
                        "* The room contains: {}\n",
                        users
                            .lock()
                            .await
                            .iter()
                            .map(String::as_ref)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .as_bytes(),
                )
                .await?;
            users.lock().await.insert(user_name.to_owned());
        } else {
            sender.send(ChatMsg::from_user(user_name.as_ref().unwrap(), msg))?;
        }
    }
    Ok(())
}

fn validate_name(name: String) -> Result<String> {
    if name.chars().any(|c| !c.is_alphanumeric()) {
        return Err(Error::InvalidName);
    }
    Ok(name)
}
