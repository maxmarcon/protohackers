mod msg;

use crate::lrcp::msg::{decode, Ack, Close, Data, Decoded};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;

#[derive(Debug)]
enum Error {
    EOF,
    IO(io::Error),
}

pub struct Socket {
    join_handle: JoinHandle<io::Result<()>>,
    stream_receiver: Receiver<Stream>,
}

impl Socket {
    pub fn new(udpsocket: UdpSocket) -> Self {
        let (stream_sender, stream_receiver) = channel(10);

        let mut socket_state = SocketState {
            udpsocket,
            session_store: HashMap::new(),
            stream_sender,
        };

        let join_handle = tokio::spawn(async move { socket_state.protocol_loop().await });

        Self {
            join_handle,
            stream_receiver,
        }
    }

    pub async fn accept(&mut self) -> Result<Stream, Error> {
        if let Some(stream) = self.stream_receiver.recv().await {
            Ok(stream)
        } else {
            Err(Error::EOF)
        }
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.join_handle.abort()
    }
}

#[derive(Debug)]
pub struct Stream {
    app_reader: Receiver<String>,
    app_writer: Sender<String>,
}

impl Stream {
    pub async fn send(&mut self, data: &str) -> Result<(), Error> {
        if self.app_writer.send(data.to_owned()).await.is_ok() {
            Ok(())
        } else {
            Err(Error::EOF)
        }
    }
}

struct Session {
    id: i32,
    acked: i32,
    peer: SocketAddr,
    pos: i32,
    to_app: Sender<String>,
    from_app: Receiver<String>,
}

struct SocketState {
    udpsocket: UdpSocket,
    session_store: HashMap<i32, Session>,
    stream_sender: Sender<Stream>,
}

impl SocketState {
    async fn protocol_loop(&mut self) -> io::Result<()> {
        loop {
            // is there data from the application layer?
            for (session_id, session) in self.session_store.iter_mut() {
                match session.from_app.try_recv() {
                    Ok(data) => {
                        self.udpsocket
                            .send_to(
                                Data::new(*session_id, session.pos, &data)
                                    .encode()
                                    .as_bytes(),
                                session.peer,
                            )
                            .await?;
                        session.pos += data.len() as i32;
                    }
                    Err(TryRecvError::Empty) => (),
                    Err(_) => {
                        // close session
                    }
                }
            }

            let mut buf = Vec::new();
            let (_size, addr) = self.udpsocket.recv_buf_from(&mut buf).await?;
            self.handle_udpdata(&buf, addr).await?;
        }
    }

    async fn ack_session(&self, session: &Session) -> io::Result<()> {
        self.udpsocket
            .send_to(
                Ack::new(session.id, session.acked).encode().as_bytes(),
                session.peer,
            )
            .await?;
        Ok(())
    }

    async fn handle_udpdata(&mut self, buf: &[u8], addr: SocketAddr) -> io::Result<()> {
        let decoded = decode(&buf);
        match decoded {
            Ok(msg) => match msg {
                Decoded::Connect(connect) => {
                    if self.session_store.get(&connect.session).is_none() {
                        let (to_app, app_reader) = channel(256);
                        let (app_writer, from_app) = channel(256);

                        let session = Session {
                            id: connect.session,
                            acked: 0,
                            pos: 0,
                            peer: addr,
                            to_app,
                            from_app,
                        };

                        let stream = Stream {
                            app_writer,
                            app_reader,
                        };
                        self.session_store.insert(connect.session, session);
                        self.ack_session(&self.session_store[&connect.session])
                            .await?;
                        self.stream_sender.send(stream).await.unwrap();
                    }
                }
                Decoded::Close(close) => {
                    if let Some(session) = self.session_store.remove(&close.session) {
                        self.udpsocket
                            .send_to(close.encode().as_bytes(), session.peer)
                            .await?;
                    }
                }
                Decoded::Data(data) => {
                    self.session_store.entry(data.session).and_modify(|s| {
                        if s.acked == data.pos {
                            s.acked += data.data.len() as i32
                        }
                    });
                    if let Some(session) = self.session_store.get(&data.session) {
                        self.ack_session(&self.session_store[&data.session]).await?;
                    } else {
                        self.udpsocket
                            .send_to(Close::new(data.session).encode().as_bytes(), addr)
                            .await?;
                    };
                }
                _ => (),
            },
            Err(_) => (),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::lrcp::{Socket, Stream};
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tokio::time;

    const SESSION: i32 = 1234567;

    #[tokio::test]
    async fn peer_opens_session() {
        open_session().await;
    }

    #[tokio::test]
    async fn peer_closes_session() {
        let (_socket, peer, _stream) = open_session().await;
        peer.send(format!("/close/{SESSION}/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/close/{SESSION}/").as_bytes());
    }

    #[tokio::test]
    async fn peer_sends_data_for_closed_session() {
        let (_socket, peer) = setup().await;
        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/close/{SESSION}/").as_bytes());
    }

    #[tokio::test]
    async fn peer_sends_data_for_open_session() {
        let (_socket, peer, _stream) = open_session().await;
        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/ack/{SESSION}/5/").as_bytes());
    }

    #[tokio::test]
    async fn peer_sends_data_beyond_what_already_acked() {
        let (_socket, peer, _stream) = open_session().await;
        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();
        peer.send(format!("/data/{SESSION}/6/world/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        buf.clear();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/ack/{SESSION}/5/").as_bytes());
    }

    #[tokio::test]
    async fn sending_data_to_peer() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/data/hello world/11/").as_bytes());
    }

    async fn open_session() -> (Socket, UdpSocket, Stream) {
        let (mut socket, peer) = setup().await;
        peer.send(format!("/connect/{SESSION}/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/ack/{SESSION}/0/").as_bytes());

        let stream = socket.accept().await;
        assert!(stream.is_ok());

        (socket, peer, stream.unwrap())
    }

    async fn setup() -> (Socket, UdpSocket) {
        let udpsocket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_addr = udpsocket.local_addr().unwrap();
        let socket = Socket::new(udpsocket);

        let udpsocket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udpsocket.connect(local_addr).await.unwrap();
        (socket, udpsocket)
    }
}
