pub use clap::Parser;
use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

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
        help = "Max concurrent connections",
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

        let mut join_handles = VecDeque::new();

        for tcp_stream in listener.incoming() {
            while join_handles.len() >= self.max_connections as usize {
                let top_thread: JoinHandle<()> = join_handles.pop_front().unwrap();
                top_thread.join().unwrap();
            }

            match tcp_stream {
                Ok(tcp_stream) => {
                    let handler = Arc::clone(&handler);
                    println!(
                        "handling connection from: {}",
                        tcp_stream.peer_addr().unwrap()
                    );
                    join_handles.push_back(thread::spawn(move || (handler)(tcp_stream)));
                }
                Err(err) => {
                    println!("Connection failed: {:?}", err);
                }
            }
        }
    }
}
