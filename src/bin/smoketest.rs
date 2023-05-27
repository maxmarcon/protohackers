use protohackers::{CliArgs, Parser, Server};
use std::io::{Read, Write};
use std::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    Server::new(handle_stream, 5).start(args.port);
}

fn handle_stream(mut stream: TcpStream) {
    let mut buf = [0; 1024];

    loop {
        let size = stream.read(&mut buf).unwrap();
        if size == 0 {
            println!("connection closed");
            return;
        }
        println!("read {size} bytes");
        stream.write_all(&buf[..size]).unwrap();
    }
}
