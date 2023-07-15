mod msg;

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

        tokio::spawn(async move {
            protocol_loop(udpsocket, to_app, from_app);
        });

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

fn protocol_loop(udpsocket: UdpSocket, to_app: Sender<String>, from_app: Receiver<String>) {}

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
        peer.send(b"/connect/1234567/");

        let mut buf = Vec::new();

        peer.recv_buf(&mut buf).await.unwrap();
        assert_eq!(buf, b"/ack/1234567/0");
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
