use protohackers::{CliArgs, Parser, Server};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

fn main() {
    let args = CliArgs::parse();

    let byte_count = Arc::new(Mutex::new(0));

    let handler: Arc<dyn Fn(TcpStream) + Send + Sync + 'static> = {
        let byte_count = Arc::clone(&byte_count);
        Arc::new(move |tcpstream| {
            handle_stream(tcpstream, &byte_count);
        })
    };

    Server::new(args.port).serve(handler);

    let byte_count = Arc::into_inner(byte_count).unwrap().into_inner().unwrap();
    println!("received a total of {} bytes", byte_count);
}

fn handle_stream(mut stream: TcpStream, byte_count: &Arc<Mutex<u32>>) {
    let mut buf = [0; 1024];
    loop {
        let size = stream.read(&mut buf).unwrap();
        if size == 0 {
            println!("connection closed");
            return;
        }

        *byte_count.lock().unwrap() += size as u32;
        stream.write_all(&buf[..size]).unwrap();
    }
}
