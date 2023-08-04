use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::Stream;
use protohackers::vcs::command;
use protohackers::vcs::command::Command;
use protohackers::{vcs, CliArgs, Parser, Server};
use std::io;
use std::sync::Arc;
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    // let (sender, _receiver) = tokio::sync::broadcast::channel(100);

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle_stream(tcpstream).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(mut tcpstream: TcpStream) -> io::Result<()> {
    let (tcpreader, mut tcpwriter) = tcpstream.split();

    // let incoming_commands = command_stream(tcpreader);

    Ok(())
}

// fn command_stream(mut tcpreader: ReadHalf) -> impl Stream<Item = command::Result<Command>> + '_ {
//     let mut buf: Vec<u8> = Vec::new();
//     try_stream! {
//         loop {
//         }
//     }
//     todo!()
// }
