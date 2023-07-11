mod async_lib;
pub mod budgetchat;

pub use clap::Parser;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use tokio::io;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Port to listen to
    #[arg(short, long, help = "Port to listen to", default_value_t = 33333)]
    pub port: u16,

    #[arg(
        short = 'c',
        long,
        help = "Serve at most these many connections in parallel",
        default_value_t = 100
    )]
    pub max_connections: u16,

    #[arg(
        short = 'u',
        long,
        help = "Max size of received UDP datagrams",
        default_value_t = 1_000
    )]
    pub max_udp_size: usize,
}

pub struct Server {
    port: u16,
    max_connections: u16,
    max_udp_size: usize,
}

impl Server {
    pub fn new(port: u16, max_connections: u16, max_udp_size: usize) -> Self {
        Self {
            port,
            max_connections,
            max_udp_size,
        }
    }

    pub fn serve_udp<F>(&self, handler: &mut F) -> io::Result<()>
    where
        F: FnMut(&[u8], &UdpSocket, SocketAddr) -> io::Result<()>,
    {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", self.port))?;
        println!("listening on: {:?}", socket.local_addr()?);
        let mut buf = vec![0; self.max_udp_size];
        loop {
            let (bytes, peer) = socket.recv_from(&mut buf)?;
            handler(&buf[..bytes], &socket, peer)?;
        }
    }

    pub fn serve<F>(&self, handler: Arc<F>) -> io::Result<()>
    where
        F: Send + Sync + 'static + ?Sized + Fn(TcpStream) -> io::Result<()>,
    {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))?;
        println!("listening on: {:?}", listener.local_addr()?);

        let mut join_handles = HashMap::new();

        let (sender, receiver) = channel();
        for (thread_id, tcp_stream) in listener.incoming().enumerate() {
            match tcp_stream {
                Ok(tcp_stream) => {
                    let sender = sender.clone();
                    let handler = handler.clone();
                    join_handles.insert(
                        thread_id,
                        thread::spawn(move || -> io::Result<()> {
                            let peer_addr = tcp_stream.peer_addr()?;
                            println!("handling connection from: {}", peer_addr);
                            let handler_result = (handler)(tcp_stream);
                            println!("done handling connection from: {}", peer_addr);
                            sender
                                .send(thread_id)
                                .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
                            handler_result
                        }),
                    );
                }
                Err(err) => {
                    println!("connection failed: {:?}", err);
                }
            }
            while join_handles.len() >= self.max_connections as usize {
                println!("maximum number of connections reached ({}) - waiting for connections to be closed", self.max_connections);
                let thread_id = receiver.recv().map_err(|_| {
                    io::Error::new(ErrorKind::BrokenPipe, "receive from thread failed")
                })?;
                if let Some(join_handle) = join_handles.remove(&thread_id) {
                    join_handle
                        .join()
                        .unwrap_or_else(|e| {
                            println!("thread failed: {:?}", e);
                            Ok(())
                        })
                        .unwrap_or_else(|e| println!("thread returned error: {:?}", e));
                }
                println!("accepting new connections again");
            }
        }
        Ok(())
    }
}
