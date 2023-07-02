use futures::future::BoxFuture;
use protohackers::{CliArgs, Parser, Server};
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    Server::new(args.port, args.max_connections)
        .serve_async(handle_stream)
        .unwrap();
}

fn handle_stream(mut tcp_stream: TcpStream) -> BoxFuture<'static, io::Result<()>> {
    Box::pin(async move {
        tcp_stream.write_all(b"Hello, what's your name?").await?;
        Ok(())
    })
}
