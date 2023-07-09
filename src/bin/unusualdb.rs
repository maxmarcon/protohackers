use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::UdpSocket;
use tokio::io;

type DbType = HashMap<String, String>;

fn main() {
    let args = CliArgs::parse();

    let mut db: DbType =
        HashMap::from([("version".to_string(), "Db version 0.1.0".to_string()); 1]);
    let mut handler =
        |bytes: &[u8], socket: &UdpSocket, peer| handle_datagram(bytes, socket, peer, &mut db);

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_udp(&mut handler)
        .unwrap();
}

fn handle_datagram(
    bytes: &[u8],
    socket: &UdpSocket,
    peer: SocketAddr,
    db: &mut DbType,
) -> io::Result<()> {
    let parts = bytes.splitn(2, |&b| b == b'=').collect::<Vec<_>>();
    match &parts[..] {
        [key, val] => {
            let key = std::str::from_utf8(key).unwrap();
            if key != "version" {
                let val = std::str::from_utf8(val).unwrap();
                db.insert(key.to_string(), val.to_string());
            }
        }
        [key] => {
            let key = std::str::from_utf8(key).unwrap();

            if let Some(val) = db.get(key) {
                let response = format!("{key}={val}");
                socket.send_to(response.as_bytes(), peer)?;
            }
        }
        [..] => panic!("unexpected"),
    }
    Ok(())
}
