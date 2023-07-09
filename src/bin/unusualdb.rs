use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io;

type DbType = HashMap<String, String>;

fn main() {
    let args = CliArgs::parse();

    let mut db: DbType = HashMap::new();
    let mut handler = |bytes: &[u8], peer| handle_datagram(bytes, peer, &mut db);

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_udp(&mut handler)
        .unwrap();
}

fn handle_datagram(bytes: &[u8], peer: SocketAddr, _db: &mut DbType) -> io::Result<()> {
    for s in bytes.splitn(2, |&b| b == b'=') {
        println!("{}", String::from_utf8(s.into()).unwrap());
    }
    Ok(())
}
