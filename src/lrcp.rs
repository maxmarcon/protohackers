mod msg;

use crate::lrcp::msg::{decode, Ack, Close, DecodeError, Decoded};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::mpsc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;

pub struct Socket {
    app_reader: Receiver<String>,
    app_writer: Sender<String>,
    join_handle: JoinHandle<io::Result<()>>,
}

impl Socket {
    pub fn new(udpsocket: UdpSocket) -> Self {
        let (to_app, app_reader) = channel(256);
        let (app_writer, from_app) = channel(256);

        let mut socket_state = SocketState {
            to_app,
            from_app,
            udpsocket,
            session_store: HashMap::new(),
        };

        let join_handle = tokio::spawn(async move { socket_state.protocol_loop().await });

        Self {
            app_reader,
            app_writer,
            join_handle,
        }
    }

    pub async fn send(&self, text: &str) {}

    pub async fn recv(&self) -> String {
        todo!()
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.join_handle.abort()
    }
}

struct Session {
    id: i32,
    acked: i32,
    peer: SocketAddr,
}

struct SocketState {
    udpsocket: UdpSocket,
    to_app: Sender<String>,
    from_app: Receiver<String>,
    session_store: HashMap<i32, Session>,
}

impl SocketState {
    async fn protocol_loop(&mut self) -> io::Result<()> {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let (_size, addr) = self.udpsocket.recv_buf_from(&mut buf).await?;
            match decode(&buf) {
                Ok(msg) => match msg {
                    Decoded::Connect(connect) => {
                        if self
                            .session_store
                            .insert(
                                connect.session,
                                Session {
                                    id: connect.session,
                                    acked: 0,
                                    peer: addr,
                                },
                            )
                            .is_none()
                        {
                            self.ack_session(&self.session_store[&connect.session])
                                .await?;
                        }
                    }
                    Decoded::Close(close) => {
                        if let Some(session) = self.session_store.remove(&close.session) {
                            println!("closing");
                            self.udpsocket
                                .send_to(close.encode().as_bytes(), session.peer)
                                .await?;
                        } else {
                            panic!("session {} not found", close.session);
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
                Err(_) => continue,
            }
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
}

#[cfg(test)]
mod tests {
    use crate::lrcp::Socket;
    use std::future::Future;
    use std::net::SocketAddr;
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
        let (socket, peer) = open_session().await;
        peer.send(format!("/close/{SESSION}/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/close/{SESSION}/").as_bytes());
    }

    #[tokio::test]
    async fn peer_sends_data_for_closed_session() {
        let (socket, peer) = setup().await;
        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/close/{SESSION}/").as_bytes());
    }

    #[tokio::test]
    async fn peer_sends_data_for_open_session() {
        let (_socket, peer) = open_session().await;

        peer.send(format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/ack/{SESSION}/5/").as_bytes());
    }

    #[tokio::test]
    async fn peer_sends_data_beyond_what_already_acked() {
        let (_socket, peer) = open_session().await;
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

    async fn open_session() -> (Socket, UdpSocket) {
        let (socket, peer) = setup().await;
        peer.send(format!("/connect/{SESSION}/").as_bytes())
            .await
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, format!("/ack/{SESSION}/0/").as_bytes());

        (socket, peer)
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
