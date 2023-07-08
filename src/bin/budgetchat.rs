use crate::Error::Disconnected;
use futures::future::BoxFuture;
use protohackers::{CliArgs, Parser, Server};
use std::fmt::{Display, Formatter};
use std::io::ErrorKind;
use std::io::ErrorKind::InvalidData;
use std::sync::Arc;
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::Sender;

fn main() {
    let args = CliArgs::parse();

    let (sender, _receiver) = broadcast::channel(100);

    let handler: Arc<dyn Send + Sync + Fn(TcpStream) -> BoxFuture<'static, io::Result<()>>> = {
        Arc::new(move |tcp_stream| {
            let sender = sender.clone();
            Box::pin(async move {
                handle_stream(tcp_stream, sender.clone(), sender.subscribe())
                    .await
                    .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))
            })
        })
    };

    Server::new(args.port, args.max_connections)
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
    mut receiver: Receiver<ChatMsg>,
) -> Result<()> {
    let mut buffer = Vec::new();
    let mut user_name = None;
    tcp_stream.write_all(b"Hello, what's your name?\n").await?;

    loop {
        tokio::select! {
            result = recv_and_process(&mut tcp_stream, &mut buffer, &mut user_name, &sender) => {
                match result {
                    Err(error) => {
                        if user_name.is_some() {
                            sender.send(ChatMsg::from_system(user_name.as_ref().unwrap(), format!("{} has left the room", user_name.as_ref().unwrap())))?;
                        }
                        return Err(error);
                    },
                    Ok(_) => (),
                }
            }
            result = receiver.recv() => {
                let chat_msg = result?;
                if !chat_msg.subject.eq(user_name.as_ref().unwrap()) {
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
) -> Result<()> {
    let read_bytes = tcp_stream.read_buf(buffer).await?;
    if read_bytes == 0 {
        return Err(Disconnected);
    }
    if let Some(msg) = parse_message(buffer)? {
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
                    format!("{} has entered the room. Kneel in front of {0}!", user_name),
                ))
                .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
        } else {
            sender.send(ChatMsg::from_user(user_name.as_ref().unwrap(), msg))?;
        }
    }
    Ok(())
}

fn parse_message(buffer: &mut Vec<u8>) -> io::Result<Option<String>> {
    if let Some(pos) = buffer
        .iter()
        .enumerate()
        .find(|(_pos, c)| **c == b'\n')
        .map(|(pos, _)| pos)
    {
        return String::from_utf8(buffer.drain(..=pos).collect())
            .map(|msg| Some(msg.trim().to_owned()))
            .map_err(|e| io::Error::new(InvalidData, e));
    }
    Ok(None)
}

fn validate_name(name: String) -> Result<String> {
    if name.chars().any(|c| !c.is_alphanumeric()) {
        return Err(Error::InvalidName);
    }
    Ok(name)
}
