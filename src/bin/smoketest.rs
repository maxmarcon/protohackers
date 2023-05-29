use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, RwLock};

fn main() {
    let args = CliArgs::parse();

    let distr = RwLock::new(HashMap::new());
    let handler: Arc<dyn Fn(TcpStream) + Send + Sync + 'static> = Arc::new(move |tcpstream| {
        handle_stream(tcpstream, &distr);
    });

    Server::new(args.port, args.max_connections).serve(handler);
}

fn handle_stream(mut stream: TcpStream, distr: &RwLock<HashMap<usize, u32>>) {
    let mut buf = [0; 1024];
    loop {
        let size = stream.read(&mut buf).unwrap();
        let mut distr = distr.write().unwrap();
        *distr.entry(size).or_default() += 1;
        if size == 0 {
            println!("connection closed");
            return;
        }
        println!("{:?}", distr);
        stream.write_all(&buf[..size]).unwrap();
    }
}
