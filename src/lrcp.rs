mod msg;

use crate::lrcp::msg::{decode, Ack, Close, Data, Decoded};
use crate::lrcp::TimeoutType::{RTX, SESSION};
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::io;
use std::net::SocketAddr;
use std::ops::Add;
use std::ops::Range;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::sleep_until;
use tokio::time::Instant;

#[derive(Debug)]
pub enum Error {
    EOF,
    Disconnected,
    IO(io::Error),
}

type Result<T> = std::result::Result<T, Error>;

pub struct Socket {
    join_handle: JoinHandle<io::Result<()>>,
    stream_receiver: Receiver<Stream>,
}

impl Socket {
    pub fn new(udpsocket: UdpSocket, rtx_to: u64, session_to: u64) -> Self {
        let (stream_sender, stream_receiver) = channel(10);

        let mut socket_state = SocketState {
            udpsocket,
            session_store: HashMap::new(),
            stream_sender,
            rtx_to,
            session_to,
            timeouts: BinaryHeap::new(),
        };

        let join_handle = tokio::spawn(async move { socket_state.protocol_loop().await });

        Self {
            join_handle,
            stream_receiver,
        }
    }

    pub async fn accept(&mut self) -> Result<Stream> {
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
    pub async fn send(&mut self, data: &str) -> Result<()> {
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

    pub async fn read(&mut self) -> Result<String> {
        let mut all_data;
        if let Some(datagram) = self.app_reader.recv().await {
            all_data = datagram.data;
        } else {
            return Err(Error::EOF);
        }
        loop {
            match self.app_reader.try_recv() {
                Ok(Datagram { data, .. }) => {
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

#[derive(Debug)]
struct Session {
    id: i32,
    // last ack sent to peer
    last_ack_sent: i32,
    peer: SocketAddr,
    // starting position of next data to send to peer
    next_pos: i32,
    to_stream: Sender<Datagram>,
    outstanding: String,
    last_seen: Option<Instant>,
}

#[derive(Debug, Clone)]
enum TimeoutType {
    RTX,
    SESSION,
}

#[derive(Debug, Clone)]
struct Timeout {
    deadline: Instant,
    session_id: i32,
    which: TimeoutType,
    data_range: Range<i32>,
}

impl PartialEq for Timeout {
    fn eq(&self, other: &Self) -> bool {
        self.deadline.eq(&other.deadline)
    }
}

impl Eq for Timeout {}

impl Ord for Timeout {
    fn cmp(&self, other: &Self) -> Ordering {
        self.deadline.cmp(&other.deadline)
    }
}

impl PartialOrd for Timeout {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.deadline.cmp(&other.deadline))
    }
}

struct SocketState {
    udpsocket: UdpSocket,
    session_store: HashMap<i32, Session>,
    stream_sender: Sender<Stream>,
    rtx_to: u64,
    session_to: u64,
    timeouts: BinaryHeap<Reverse<Timeout>>,
}

impl SocketState {
    async fn protocol_loop(&mut self) -> io::Result<()> {
        let (app_writer, mut from_stream) = channel(256);
        loop {
            let mut buf = Vec::new();

            let next_timeout = self.timeouts.peek().map(|Reverse(timeout)| timeout);
            tokio::select! {
                _ = async { sleep_until(next_timeout.unwrap().deadline).await }, if next_timeout.is_some() => {
                    let timeout = self.timeouts.pop().unwrap().0;
                    match timeout.which {
                        RTX => {
                            self.handle_rtx_to(timeout).await?
                        },
                        SESSION => {
                            self.handle_session_to(timeout).await?
                        }
                    }
                }
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
                        self.timeouts.push(std::cmp::Reverse(Timeout{
                            deadline: Instant::now().add(Duration::from_millis(self.rtx_to)),
                            which: RTX,
                            session_id: session.id,
                            data_range: session.next_pos..session.next_pos+datagram.data.len() as i32
                        }));
                        session.next_pos += datagram.data.len() as i32;
                        session.outstanding += &datagram.data;
                    }
                }
            }
        }
    }

    async fn handle_rtx_to(&mut self, timeout: Timeout) -> io::Result<()> {
        let Timeout {
            data_range,
            session_id,
            ..
        } = &timeout;
        if let Some(session) = self.session_store.get(&session_id) {
            if session.next_pos as usize - session.outstanding.len() <= data_range.start as usize {
                let outstading_start_pos = session.next_pos - session.outstanding.len() as i32;
                let data = Data::new(
                    *session_id,
                    data_range.start,
                    &session.outstanding[data_range.start as usize - outstading_start_pos as usize
                        ..data_range.end as usize - outstading_start_pos as usize],
                );
                self.udpsocket
                    .send_to(data.encode().as_bytes(), session.peer)
                    .await?;

                let mut new_timeout = timeout.clone();
                new_timeout.deadline = Instant::now().add(Duration::from_millis(self.rtx_to));
                self.timeouts.push(Reverse(new_timeout));
            }
        }
        Ok(())
    }

    async fn handle_session_to(
        &mut self,
        Timeout {
            session_id,
            deadline,
            ..
        }: Timeout,
    ) -> io::Result<()> {
        if self.session_store.get(&session_id).is_some_and(|session| {
            session
                .last_seen
                .is_some_and(|last_seen| &deadline > &last_seen)
        }) {
            self.close_session(session_id).await?;
        }
        Ok(())
    }

    async fn close_session(&mut self, session_id: i32) -> io::Result<()> {
        if let Some(session) = self.session_store.remove(&session_id) {
            self.udpsocket
                .send_to(Close::new(session_id).encode().as_bytes(), session.peer)
                .await?;
        }
        Ok(())
    }

    async fn handle_ack(
        &mut self,
        session_id: i32,
        new_ack: i32,
        peer: SocketAddr,
    ) -> io::Result<()> {
        let session = self.session_store.get(&session_id);
        if session.is_none() {
            self.udpsocket
                .send_to(Close::new(session_id).encode().as_bytes(), peer)
                .await?;
        }

        let session = session.unwrap();

        let first_outstanding = session.next_pos - session.outstanding.len() as i32;
        if new_ack > session.next_pos {
            return self.close_session(session.id).await;
        }
        if new_ack < first_outstanding {
            return Ok(());
        }

        let session = self.session_store.get_mut(&session_id).unwrap();

        let acked = new_ack - first_outstanding;
        session.outstanding.drain(..acked as usize);
        if session.outstanding.len() > 0 {
            // Partial ack
            let data = Data::new(session.id, new_ack, &session.outstanding);
            self.udpsocket
                .send_to(data.encode().as_bytes(), session.peer)
                .await?;
        }
        Ok(())
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

    fn update_last_seen(&mut self, session_id: i32) {
        if let Some(session) = self.session_store.get_mut(&session_id) {
            session.last_seen = Some(Instant::now());
            let timeout = Timeout {
                which: SESSION,
                session_id: session.id,
                data_range: 0..0,
                deadline: Instant::now().add(Duration::from_millis(self.session_to)),
            };
            self.timeouts.push(Reverse(timeout));
        }
    }

    async fn new_session(
        &mut self,
        session_id: i32,
        peer: SocketAddr,
        app_writer: Sender<Datagram>,
    ) -> io::Result<()> {
        let (to_app, app_reader) = channel(256);

        let session = Session {
            id: session_id,
            last_ack_sent: 0,
            next_pos: 0,
            peer,
            to_stream: to_app,
            outstanding: String::new(),
            last_seen: None,
        };

        let stream = Stream {
            session_id: session.id,
            app_writer: app_writer,
            app_reader,
        };
        self.session_store.insert(session_id, session);
        self.ack_session(&self.session_store[&session_id]).await?;
        self.stream_sender.send(stream).await.unwrap();

        Ok(())
    }

    async fn handle_data(&mut self, data: Data, peer: SocketAddr) -> io::Result<()> {
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
            self.ack_session(&session).await?;
        } else {
            self.udpsocket
                .send_to(Close::new(data.session).encode().as_bytes(), peer)
                .await?;
        }
        Ok(())
    }

    async fn process_udp(
        &mut self,
        buf: &[u8],
        peer: SocketAddr,
        app_writer: &Sender<Datagram>,
    ) -> io::Result<()> {
        match decode(&buf) {
            Ok(msg) => match msg {
                Decoded::Connect(connect) => {
                    if self.session_store.get(&connect.session).is_none() {
                        self.new_session(connect.session, peer, app_writer.clone())
                            .await?;
                    }
                }
                Decoded::Close(close) => {
                    self.close_session(close.session).await?;
                }
                Decoded::Data(data) => {
                    self.update_last_seen(data.session);
                    self.handle_data(data, peer).await?;
                }
                Decoded::Ack(ack) => {
                    self.update_last_seen(ack.session);
                    self.handle_ack(ack.session, ack.length, peer).await?;
                }
            },
            Err(_) => (),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::lrcp::{Error, Socket, Stream};
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tokio::time::timeout;

    const SESSION: i32 = 1234567;
    const RTX_TO: u64 = 3_000;
    const SESSION_TO: u64 = 60_000;

    #[tokio::test]
    async fn peer_opens_session() {
        open_session().await;
    }

    #[tokio::test]
    async fn peer_closes_session() {
        let (_socket, peer, _stream) = open_session().await;
        peer.send(&format!("/close/{SESSION}/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/close/{SESSION}/")).await;
    }

    #[tokio::test]
    async fn peer_sends_data_for_closed_session() {
        let (_socket, peer) = setup().await;
        peer.send(&format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/close/{SESSION}/")).await;
    }

    #[tokio::test]
    async fn peer_sends_data_for_open_session() {
        let (_socket, peer, mut stream) = open_session().await;
        peer.send(&format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/ack/{SESSION}/5/")).await;

        let read_result = stream.read().await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn peer_sends_data_beyond_what_already_acked() {
        let (_socket, peer, mut stream) = open_session().await;
        peer.send(&format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();
        peer.send(&format!("/data/{SESSION}/6/world/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/ack/{SESSION}/5/")).await;
        assert_receive(&peer, &format!("/ack/{SESSION}/5/")).await;

        let read_result = stream.read().await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn multiple_data_packets_from_peer_are_combined_in_stream() {
        let (_socket, peer, mut stream) = open_session().await;
        peer.send(&format!("/data/{SESSION}/0/hello/").as_bytes())
            .await
            .unwrap();
        peer.send(&format!("/data/{SESSION}/5/ world/").as_bytes())
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

        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;
    }

    #[tokio::test]
    async fn retransmission() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());
        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;

        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(RTX_TO)).await;

        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;
    }

    #[tokio::test]
    async fn session_timeout() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());

        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;
        peer.send(&format!("/ack/{SESSION}/11/").as_bytes())
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(SESSION_TO)).await;

        assert_eventually_receive(&peer, &format!("/close/{SESSION}/")).await;

        let read = stream.read().await;
        assert!(read.is_err());
        assert!(matches!(read.unwrap_err(), Error::EOF));
    }

    #[tokio::test]
    async fn peer_sends_ack_for_closed_session() {
        let (_socket, peer, _stream) = open_session().await;

        peer.send(b"/ack/99999/10/").await.unwrap();

        assert_receive(&peer, "/close/99999/").await;
    }

    #[tokio::test]
    async fn peer_sends_ack_for_unsent_data() {
        let (_socket, peer, _stream) = open_session().await;

        peer.send(&format!("/ack/{SESSION}/10/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/close/{SESSION}/")).await;
    }

    #[tokio::test]
    async fn peer_sends_partial_acks() {
        let (_socket, peer, mut stream) = open_session().await;

        stream.send("hello world").await.unwrap();

        peer.send(&format!("/ack/{SESSION}/5/").as_bytes())
            .await
            .unwrap();

        assert_eventually_receive(&peer, &&format!("/data/{SESSION}/5/ world/")).await;

        peer.send(&format!("/ack/{SESSION}/7/").as_bytes())
            .await
            .unwrap();

        assert_eventually_receive(&peer, &&format!("/data/{SESSION}/7/orld/")).await;
    }

    async fn open_session() -> (Socket, UdpSocket, Stream) {
        let (mut socket, peer) = setup().await;
        peer.send(&format!("/connect/{SESSION}/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/ack/{SESSION}/0/")).await;

        let stream = socket.accept().await;
        assert!(stream.is_ok());

        (socket, peer, stream.unwrap())
    }

    async fn setup() -> (Socket, UdpSocket) {
        let udpsocket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_addr = udpsocket.local_addr().unwrap();
        let socket = Socket::new(udpsocket, RTX_TO, SESSION_TO);

        let udpsocket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udpsocket.connect(local_addr).await.unwrap();
        (socket, udpsocket)
    }

    async fn assert_receive(peer: &UdpSocket, expected: &str) {
        let mut buf: Vec<u8> = Vec::new();
        let recv_timeout = 2_000;
        match timeout(Duration::from_millis(recv_timeout), peer.recv_buf(&mut buf)).await {
            Ok(_) => assert_eq!(String::from_utf8(buf).unwrap(), expected),
            Err(_) => panic!("did not receive anything withing {recv_timeout} msec"),
        }
    }

    async fn assert_eventually_receive(peer: &UdpSocket, expected: &str) {
        let receive_loop = async {
            loop {
                let mut buf: Vec<u8> = Vec::new();
                peer.recv_buf(&mut buf).await.unwrap();
                if String::from_utf8(buf).unwrap() == expected {
                    return;
                };
            }
        };
        let recv_timeout = 2_000;

        match timeout(Duration::from_millis(2_000), receive_loop).await {
            Ok(_) => (),
            Err(_) => panic!("did not receive anything withing {recv_timeout} msec"),
        }
    }
}
