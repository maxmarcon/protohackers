use futures::future::BoxFuture;
use md5::{Context, Digest};
use protohackers::vcs;
use protohackers::vcs::command;
use protohackers::vcs::command::Command;
use protohackers::{CliArgs, Parser, Server};
use std::cmp::min;
use std::env::temp_dir;
use std::io;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use uuid::Uuid;

const MAX_FILE_SIZE: usize = 10_000_000;

static HELP_MSG: &str = "OK usage: HELP|GET|PUT|LIST";

fn main() {
    let args = CliArgs::parse();
    vcs::create_working_dir().unwrap();

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle_stream(tcpstream).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(tcpstream: TcpStream) -> io::Result<()> {
    todo!()
}
