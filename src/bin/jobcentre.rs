use futures::future::BoxFuture;
use protohackers::{CliArgs, Parser, Server};
use std::sync::Arc;
use tokio::io;
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle_stream(tcpstream).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(mut tcpstream: TcpStream) -> io::Result<()> {
    let (tcpreader, tcpwriter) = tcpstream.split();

    Ok(())
}
