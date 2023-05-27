pub use clap::Parser;
use std::net::{TcpListener, TcpStream};
use std::thread;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Port to listen to
    #[arg(short, long, help = "Port to listen to", default_value_t = 3333)]
    pub port: u16,
}

pub struct Server {
    max_connections: u16,
    handler: fn(TcpStream),
}

impl Server {
    pub fn new(handler: fn(TcpStream), max_connections: u16) -> Self {
        Self {
            handler,
            max_connections,
        }
    }

    pub fn start(&mut self, port: u16) {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).unwrap();
        println!("listening on: {:?}", listener.local_addr().unwrap());

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!("handling connection from: {}", stream.peer_addr().unwrap());
                    let handler = self.handler.clone();
                    thread::spawn(move || (handler)(stream));
                }
                Err(err) => {
                    println!("Connection failed: {:?}", err);
                }
            }
        }
    }
}
