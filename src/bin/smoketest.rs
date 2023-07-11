use protohackers::{CliArgs, Parser, Server};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use tokio::io;

fn main() {
    let args = CliArgs::parse();

    let byte_count = Arc::new(Mutex::new(0));

    let handler: Arc<_> = {
        let byte_count = Arc::clone(&byte_count);
        Arc::new(move |tcpstream| handle_stream(tcpstream, &byte_count))
    };

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve(handler)
        .unwrap();

    let byte_count = Arc::into_inner(byte_count).unwrap().into_inner().unwrap();
    println!("received a total of {} bytes", byte_count);
}

fn handle_stream(mut stream: TcpStream, byte_count: &Arc<Mutex<u32>>) -> io::Result<()> {
    let mut buf = [0; 1024];
    loop {
        let size = stream.read(&mut buf)?;
        if size == 0 {
            println!("connection closed");
            break;
        }

        *byte_count.lock().unwrap() += size as u32;
        stream.write_all(&buf[..size])?;
    }
    Ok(())
}
