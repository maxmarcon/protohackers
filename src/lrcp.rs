mod msg;

use crate::lrcp::msg::{decode, Ack, DecodeError, Decoded};
use std::io;
use std::sync::mpsc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct Socket {
    app_reader: Receiver<String>,
    app_writer: Sender<String>,
}

impl Socket {
    pub fn new(udpsocket: UdpSocket) -> Self {
        let (to_app, app_reader) = channel(256);
        let (app_writer, from_app) = channel(256);

        tokio::spawn(async move { protocol_loop(udpsocket, to_app, from_app).await });

        Self {
            app_reader,
            app_writer,
        }
    }

    pub async fn send(&self, text: &str) {}

    pub async fn recv(&self) -> String {
        todo!()
    }
}

async fn protocol_loop(
    udpsocket: UdpSocket,
    to_app: Sender<String>,
    from_app: Receiver<String>,
) -> io::Result<()> {
    let mut buf = Vec::new();
    loop {
        let (_size, addr) = udpsocket.recv_buf_from(&mut buf).await?;
        match decode(&buf) {
            Ok(msg) => match msg {
                Decoded::Connect(connect) => {
                    udpsocket
                        .send_to(Ack::new(connect.session, 0).encode().as_bytes(), addr)
                        .await?;
                }
                _ => (),
            },
            Err(_) => continue,
        }
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

    #[tokio::test]
    async fn open_connection() {
        let (socket, peer) = setup().await;
        peer.send(b"/connect/1234567/").await.unwrap();

        let mut buf: Vec<u8> = Vec::new();
        peer.recv_buf(&mut buf).await.unwrap();
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
