use protohackers::{CliArgs, Parser, Server};
use std::collections::HashMap;
use std::net::UdpSocket;
use tokio::io;

type DbType = HashMap<String, String>;

fn main() {
    let args = CliArgs::parse();

    let mut db: DbType =
        HashMap::from([("version".to_string(), "Db version 0.1.0".to_string()); 1]);
    let mut handler =
        |socket: UdpSocket, max_udp_size: usize| receive_loop(socket, max_udp_size, &mut db);

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_udp(&mut handler)
        .unwrap();
}

fn receive_loop(socket: UdpSocket, max_udp_size: usize, db: &mut DbType) -> io::Result<()> {
    let mut buf = vec![0; max_udp_size];
    loop {
        let (received, peer) = socket.recv_from(&mut buf)?;
        let parts = buf[..received]
            .splitn(2, |&b| b == b'=')
            .collect::<Vec<_>>();
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
    }
}
