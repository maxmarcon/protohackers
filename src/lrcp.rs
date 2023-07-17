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
pub enum Error {
    EOF,
    Disconnected,
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
            Err(Error::Disconnected)
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
    session_id: i32,
    app_reader: Receiver<Datagram>,
    app_writer: Sender<Datagram>,
}

impl Stream {
    pub async fn send(&mut self, data: &str) -> Result<(), Error> {
        if self
            .app_writer
            .send(Datagram {
                session_id: self.session_id,
                data: data.to_owned(),
            })
            .await
            .is_ok()
        {
            Ok(())
        } else {
            Err(Error::Disconnected)
        }
    }

    pub async fn read(&mut self) -> Result<String, Error> {
        let mut all_data;
        if let Some(datagram) = self.app_reader.recv().await {
            all_data = datagram.data;
        } else {
            return Err(Error::EOF);
        }
        loop {
            match self.app_reader.try_recv() {
                Ok(Datagram { data, session_id }) => {
                    all_data += &data;
                }
                Err(TryRecvError::Empty) => break,
                Err(_) => return Err(Error::EOF),
            }
        }
        Ok(all_data)
    }
}

#[derive(Debug, Clone)]
struct Datagram {
    pub data: String,
    pub session_id: i32,
}

struct Session {
    id: i32,
    // last ack sent to peer
    last_ack_sent: i32,
    peer: SocketAddr,
    // starting position of next data to send to peer
    next_pos: i32,
    to_stream: Sender<Datagram>,
}

struct SocketState {
    udpsocket: UdpSocket,
    session_store: HashMap<i32, Session>,
    stream_sender: Sender<Stream>,
}

impl SocketState {
    async fn protocol_loop(&mut self) -> io::Result<()> {
        let (app_writer, mut from_stream) = channel(256);
        loop {
            let mut buf = Vec::new();
            tokio::select! {
                result = self.udpsocket.recv_buf_from(&mut buf) => {
                    let (_size, addr) = result?;
                    self.process_udp(&buf, addr, &app_writer).await?;
                }
                Some(datagram) = from_stream.recv() => {
                     if let Some(session) = self.session_store.get_mut(&datagram.session_id) {
                         self.udpsocket
                                .send_to(
                                    Data::new(session.id, session.next_pos, &datagram.data)
                                        .encode()
                                        .as_bytes(),
                                    session.peer,
                                )
                                .await?;
                        session.next_pos += datagram.data.len() as i32;
                    }
                }
            }
        }
    }

    async fn ack_session(&self, session: &Session) -> io::Result<()> {
        self.udpsocket
            .send_to(
                Ack::new(session.id, session.last_ack_sent)
                    .encode()
                    .as_bytes(),
                session.peer,
            )
            .await?;
        Ok(())
    }

    async fn process_udp(
        &mut self,
        buf: &[u8],
        addr: SocketAddr,
        app_writer: &Sender<Datagram>,
    ) -> io::Result<()> {
        let decoded = decode(&buf);
        match decoded {
            Ok(msg) => match msg {
                Decoded::Connect(connect) => {
                    if self.session_store.get(&connect.session).is_none() {
                        let (to_app, app_reader) = channel(256);

                        let session = Session {
                            id: connect.session,
                            last_ack_sent: 0,
                            next_pos: 0,
                            peer: addr,
                            to_stream: to_app,
                        };

                        let stream = Stream {
                            session_id: session.id,
                            app_writer: app_writer.clone(),
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
                    let mut stream_gone = false;
                    if let Some(session) = self.session_store.get_mut(&data.session) {
                        if session.last_ack_sent == data.pos {
                            session.last_ack_sent += data.data.len() as i32;
                            stream_gone = session
                                .to_stream
                                .send(Datagram {
                                    session_id: session.id,
                                    data: data.data,
                                })
                                .await
                                .is_err();
                        }
                    };
                    if stream_gone {
                        self.session_store.remove(&data.session);
                    }
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
        let (_socket, peer, mut stream) = open_session().await;
        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/ack/{SESSION}/5/").as_bytes());

        let read_result = stream.read().await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn peer_sends_data_beyond_what_already_acked() {
        let (_socket, peer, mut stream) = open_session().await;
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

        let read_result = stream.read().await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn multiple_data_packets_from_peer_are_combined_in_stream() {
        let (_socket, peer, mut stream) = open_session().await;
        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();
        peer.send(format!("/data/{SESSION}/5/ world/").as_bytes())
            .await
            .unwrap();

        let read_result = stream.read().await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "hello world");
    }

    #[tokio::test]
    async fn local_end_sends_data_to_peer() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            format!("/data/{SESSION}/0/hello world/")
        );
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
