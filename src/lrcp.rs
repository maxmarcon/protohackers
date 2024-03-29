mod msg;

use crate::lrcp::msg::{decode, Ack, Close, Data, Decoded};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::ops::Add;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::time::{sleep, sleep_until};

#[derive(Debug)]
pub enum Error {
    Eof,
    Disconnected,
    Io(io::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Eof => writeln!(f, "EOF"),
            Error::Disconnected => writeln!(f, "Disconnected"),
            Error::Io(io_error) => writeln!(f, "{}", io_error),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for io::Error {
    fn from(error: Error) -> Self {
        io::Error::new(ErrorKind::Other, error)
    }
}

pub struct Socket {
    join_handle: JoinHandle<()>,
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
        };

        let join_handle = tokio::spawn(async move { socket_state.protocol_loop().await.unwrap() });

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
    pub session_id: i32,
    reader: Receiver<Datagram>,
    writer: Sender<Datagram>,
}

impl Stream {
    pub async fn send(&mut self, data: &str) -> Result<()> {
        if self
            .writer
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
        if let Some(datagram) = self.reader.recv().await {
            all_data = datagram.data;
        } else {
            return Err(Error::Eof);
        }
        loop {
            match self.reader.try_recv() {
                Ok(datagram) => {
                    all_data += &datagram.data;
                }
                Err(TryRecvError::Empty) => break,
                Err(_) => return Err(Error::Eof),
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
    bytes_acked: i32,
    peer: SocketAddr,
    // starting position of next data to send to peer
    bytes_sent: i32,
    to_stream: Sender<Datagram>,
    outstanding: String,
    rtx_to: Option<Instant>,
    session_to: Option<Instant>,
}

impl Display for Session {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let rtx_in = self
            .rtx_to
            .map(|t| t.duration_since(Instant::now()).as_millis());
        let session_in = self
            .session_to
            .map(|t| t.duration_since(Instant::now()).as_millis());

        write!(
            f,
            "id={} last_ack_sent={} next_pos={} oustanding={} rtx_to={:?} session_to={:?}",
            self.id,
            self.bytes_acked,
            self.bytes_sent,
            self.outstanding.len(),
            rtx_in,
            session_in
        )
    }
}

impl Session {
    fn timeouts(&self) -> Vec<Timeout> {
        [
            self.rtx_to.map(|t| Timeout {
                deadline: t,
                session_id: self.id,
                which: TimeoutType::Rtx,
            }),
            self.session_to.map(|t| Timeout {
                deadline: t,
                session_id: self.id,
                which: TimeoutType::Session,
            }),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn reset_session_to(&mut self, session_to: u64) {
        self.session_to = Some(Instant::now().add(Duration::from_millis(session_to)))
    }

    fn cancel_session_to(&mut self) {
        self.session_to = None
    }

    fn reset_rtx_to(&mut self, rtx_to: u64) {
        self.rtx_to = Some(Instant::now().add(Duration::from_millis(rtx_to)))
    }

    fn cancel_rtx_to(&mut self) {
        self.rtx_to = None
    }
}

#[derive(Debug, Clone)]
enum TimeoutType {
    Rtx,
    Session,
}

#[derive(Debug, Clone)]
struct Timeout {
    deadline: Instant,
    session_id: i32,
    which: TimeoutType,
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

const MAX_DATA_LEN: usize = 400;

struct SocketState {
    udpsocket: UdpSocket,
    session_store: HashMap<i32, Session>,
    stream_sender: Sender<Stream>,
    rtx_to: u64,
    session_to: u64,
}

impl SocketState {
    pub fn timeouts(&self) -> impl Iterator<Item = Timeout> + '_ {
        self.session_store.values().flat_map(Session::timeouts)
    }
}

impl SocketState {
    async fn protocol_loop(&mut self) -> io::Result<()> {
        let (stream_writer, mut from_stream) = channel(256);
        let mut buf = vec![0; 65536];
        loop {
            let next_timeout = self.timeouts().min();

            tokio::select! {
                _ = async { sleep_until(next_timeout.as_ref().unwrap().deadline).await }, if next_timeout.is_some() => {
                    let timeout = next_timeout.unwrap();
                    match timeout.which {
                        TimeoutType::Rtx => {
                            self.handle_rtx_timeout(timeout.session_id).await?
                        },
                        TimeoutType::Session => {
                            self.close_session(timeout.session_id, None).await?;
                        }
                    }
                }
                result = self.udpsocket.recv_from(&mut buf) => {
                    let (size, addr) = result?;
                    self.process_udp(&buf[..size], addr, &stream_writer).await?;
                }
                Some(datagram) = from_stream.recv() => {
                    if let Some(session) = self.session_store.get_mut(&datagram.session_id) {
                         Self::send_data(session.id, session.bytes_sent, &datagram.data, &self.udpsocket, session.peer).await?;
                         session.bytes_sent += datagram.data.len() as i32;
                         session.outstanding += &datagram.data;
                         session.reset_rtx_to(self.rtx_to);
                         session.reset_session_to(self.session_to);
                    }
                }
            }
        }
    }

    async fn handle_rtx_timeout(&mut self, session_id: i32) -> io::Result<()> {
        if let Some(session) = self.session_store.get_mut(&session_id) {
            if !session.outstanding.is_empty() {
                Self::send_data(
                    session.id,
                    session.bytes_sent - session.outstanding.len() as i32,
                    &session.outstanding,
                    &self.udpsocket,
                    session.peer,
                )
                .await?;
                session.rtx_to = Some(Instant::now().add(Duration::from_millis(self.rtx_to)));
            } else {
                session.rtx_to = None;
            }
        }
        Ok(())
    }

    async fn close_session(&mut self, session_id: i32, peer: Option<SocketAddr>) -> io::Result<()> {
        let session = self.session_store.remove(&session_id);
        let peer = peer.unwrap_or_else(|| session.unwrap().peer);
        println!("closing session {session_id}");
        self.udpsocket
            .send_to(Close::new(session_id).encode().as_bytes(), peer)
            .await?;
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
            self.close_session(session_id, Some(peer)).await?;
            return Ok(());
        }

        let session = session.unwrap();

        let first_outstanding = session.bytes_sent - session.outstanding.len() as i32;
        if new_ack > session.bytes_sent {
            return self.close_session(session.id, None).await;
        }

        if new_ack <= first_outstanding {
            return Ok(());
        }

        let session = self.session_store.get_mut(&session_id).unwrap();
        let acked = new_ack - first_outstanding;
        session.outstanding.drain(..acked as usize);

        if session.outstanding.is_empty() {
            session.cancel_session_to();
            session.cancel_rtx_to();
        } else {
            // Partial ack
            Self::send_data(
                session.id,
                new_ack,
                &session.outstanding,
                &self.udpsocket,
                session.peer,
            )
            .await?;
            session.reset_rtx_to(self.rtx_to);
        }
        Ok(())
    }

    async fn send_data(
        session_id: i32,
        pos: i32,
        data: &str,
        udpsocket: &UdpSocket,
        to: SocketAddr,
    ) -> io::Result<()> {
        let mut pieces = Vec::new();
        let mut remaining = data;
        let mut chunk;
        while remaining.len() > MAX_DATA_LEN {
            (chunk, remaining) = remaining.split_at(MAX_DATA_LEN);
            pieces.push(chunk);
        }
        pieces.push(remaining);

        let mut current_pos = pos;
        for piece in pieces {
            let data = Data::new(session_id, current_pos, piece);
            // sleep a few millisecs to avoid the "No buffer space available" error
            sleep(Duration::from_millis(10)).await;
            udpsocket.send_to(data.encode().as_bytes(), to).await?;
            current_pos += piece.len() as i32;
        }
        Ok(())
    }

    async fn ack_session(&self, session: &Session) -> io::Result<()> {
        self.udpsocket
            .send_to(
                Ack::new(session.id, session.bytes_acked)
                    .encode()
                    .as_bytes(),
                session.peer,
            )
            .await?;
        Ok(())
    }

    async fn new_session(
        &mut self,
        session_id: i32,
        peer: SocketAddr,
        stream_writer: Sender<Datagram>,
    ) -> io::Result<()> {
        let (to_stream, stream_reader) = channel(256);

        let session = Session {
            id: session_id,
            bytes_acked: 0,
            bytes_sent: 0,
            peer,
            to_stream,
            outstanding: String::new(),
            rtx_to: None,
            session_to: None,
        };

        let stream = Stream {
            session_id: session.id,
            writer: stream_writer,
            reader: stream_reader,
        };
        println!("new session {session_id}");
        self.session_store.insert(session_id, session);
        self.ack_session(&self.session_store[&session_id]).await?;
        self.stream_sender.send(stream).await.unwrap();
        Ok(())
    }

    async fn handle_data(&mut self, data: Data, peer: SocketAddr) -> io::Result<()> {
        let mut stream_gone = false;
        if let Some(session) = self.session_store.get_mut(&data.session) {
            if session.bytes_acked == data.pos {
                session.bytes_acked += data.data.len() as i32;
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
            self.ack_session(session).await?;
        } else {
            self.close_session(data.session, Some(peer)).await?;
        }
        Ok(())
    }

    async fn process_udp(
        &mut self,
        buf: &[u8],
        peer: SocketAddr,
        stream_writer: &Sender<Datagram>,
    ) -> io::Result<()> {
        if let Ok(msg) = decode(buf) {
            match msg {
                Decoded::Connect(connect) => {
                    if let Some(session) = self.session_store.get(&connect.session) {
                        self.ack_session(session).await?;
                    } else {
                        self.new_session(connect.session, peer, stream_writer.clone())
                            .await?;
                    }
                }
                Decoded::Close(close) => {
                    self.close_session(close.session, Some(peer)).await?;
                }
                Decoded::Data(data) => {
                    self.handle_data(data, peer).await?;
                }
                Decoded::Ack(ack) => {
                    self.handle_ack(ack.session, ack.length, peer).await?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::lrcp::{Error, Socket, Stream, MAX_DATA_LEN};
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tokio::time::{sleep, timeout};

    const SESSION: i32 = 1234567;
    const RTX_TO: u64 = 3_000;
    const SESSION_TO: u64 = 60_000;

    #[tokio::test]
    async fn peer_opens_session() {
        open_session().await;
    }

    #[tokio::test]
    async fn repeated_connection_message() {
        let (_socket, peer, _stream) = open_session().await;

        peer.send(format!("/connect/{SESSION}/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/ack/{SESSION}/0/")).await;
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
        peer.send(&format!("/data/{SESSION}/0/hello \n mmm/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/ack/{SESSION}/11/")).await;

        let read_result = stream.read().await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "hello \n mmm");
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
    async fn retransmission_timeout_for_partially_acked_data() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());
        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;

        peer.send(&format!("/ack/{SESSION}/6/").as_bytes())
            .await
            .unwrap();

        assert_receive(&peer, &format!("/data/{SESSION}/6/world/")).await;

        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(RTX_TO)).await;

        assert_receive(&peer, &format!("/data/{SESSION}/6/world/")).await;
    }

    #[tokio::test]
    async fn session_timeout() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());

        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;

        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(SESSION_TO)).await;

        assert_eventually_receive(&peer, &format!("/close/{SESSION}/")).await;

        let read = stream.read().await;
        assert!(read.is_err());
        assert!(matches!(read.unwrap_err(), Error::Eof));
    }

    #[tokio::test]
    async fn session_not_closed_if_no_ack_expected() {
        let (_socket, peer, mut stream) = open_session().await;

        assert!(stream.send("hello world").await.is_ok());

        assert_receive(&peer, &format!("/data/{SESSION}/0/hello world/")).await;

        peer.send(format!("/ack/{SESSION}/11/").as_bytes())
            .await
            .unwrap();

        // give time to server to process the ack
        sleep(Duration::from_millis(10)).await;

        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(SESSION_TO)).await;

        refute_eventually_receive(&peer, &format!("/close/{SESSION}/")).await;
    }

    #[tokio::test]
    async fn peer_sends_ack_for_closed_session() {
        let (_socket, peer, _stream) = open_session().await;

        peer.send(b"/ack/99999/10/").await.unwrap();

        assert_receive(&peer, "/close/99999/").await;
    }

    #[tokio::test]
    async fn peer_sends_ack_for_unsent_data() {
        let (_socket, peer, mut stream) = open_session().await;

        stream.send("hello").await.unwrap();
        assert_receive(&peer, &format!("/data/{SESSION}/0/hello/")).await;

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

    #[tokio::test]
    async fn large_datagrams_splits_into_multiple_messages() {
        let (_socket, peer, mut stream) = open_session().await;

        let data: String = (0..MAX_DATA_LEN * 2).map(|_| 'X').collect();

        stream.send(&data).await.unwrap();

        assert_receive_with_size(&peer, MAX_DATA_LEN + 4 + 7 + 1 + 5).await;
        assert_receive_with_size(&peer, MAX_DATA_LEN + 4 + 7 + 3 + 5).await;
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
        let mut buf = vec![0; 1024];
        let recv_timeout = 2_000;
        match timeout(Duration::from_millis(recv_timeout), peer.recv(&mut buf)).await {
            Ok(Ok(size)) => assert_eq!(String::from_utf8(buf[..size].to_vec()).unwrap(), expected),
            Ok(Err(error)) => panic!("{}", error),
            Err(_) => panic!("did not receive anything withing {recv_timeout} msec"),
        }
    }

    async fn assert_receive_with_size(peer: &UdpSocket, expected_size: usize) {
        let mut buf = vec![0; 1024];
        let recv_timeout = 2_000;
        match timeout(Duration::from_millis(recv_timeout), peer.recv(&mut buf)).await {
            Ok(Ok(size)) => assert_eq!(size, expected_size),
            Ok(Err(error)) => panic!("{}", error),
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

        match timeout(Duration::from_millis(recv_timeout), receive_loop).await {
            Ok(_) => (),
            Err(_) => panic!("did not receive \"{expected}\" within {recv_timeout} msec"),
        }
    }

    async fn refute_eventually_receive(peer: &UdpSocket, expected: &str) {
        let receive_loop = async {
            loop {
                let mut buf: Vec<u8> = Vec::new();
                peer.recv_buf(&mut buf).await.unwrap();
                if String::from_utf8(buf).unwrap() == expected {
                    panic!("received unexpected message {expected}");
                };
            }
        };
        let recv_timeout = 2_000;

        let _ = timeout(Duration::from_millis(recv_timeout), receive_loop).await;
    }
}
