mod async_lib;

pub use clap::Parser;
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;

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
        default_value_t = 5
    )]
    pub max_connections: u16,
}

pub struct Server {
    port: u16,
    max_connections: u16,
}

impl Server {
    pub fn new(port: u16, max_connections: u16) -> Self {
        Self {
            port,
            max_connections,
        }
    }

    pub fn serve(&self, handler: Arc<dyn Fn(TcpStream) + Send + Sync>) {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port)).unwrap();
        println!("listening on: {:?}", listener.local_addr().unwrap());

        let mut join_handles = HashMap::new();

        let (sender, receiver) = channel();
        for (thread_id, tcp_stream) in listener.incoming().enumerate() {
            match tcp_stream {
                Ok(tcp_stream) => {
                    let handler = Arc::clone(&handler);
                    let sender = sender.clone();
                    join_handles.insert(
                        thread_id,
                        thread::spawn(move || {
                            let peer_addr = tcp_stream.peer_addr().unwrap();
                            println!("handling connection from: {}", peer_addr);
                            (handler)(tcp_stream);
                            println!("done handling connection from: {}", peer_addr);
                            sender.send(thread_id).unwrap();
                        }),
                    );
                }
                Err(err) => {
                    println!("connection failed: {:?}", err);
                }
            }
            while join_handles.len() >= self.max_connections as usize {
                println!("maximum number of connections reached ({}) - waiting for connections to be closed", self.max_connections);
                let thread_id = match receiver.recv() {
                    Ok(thread_id) => thread_id,
                    Err(_) => break,
                };
                if let Some(join_handle) = join_handles.remove(&thread_id) {
                    join_handle.join().unwrap();
                }
                println!("accepting new connections again");
            }
        }
    }
}
