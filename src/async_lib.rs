use futures::future::BoxFuture;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::spawn;
use tokio::sync::mpsc::channel;

impl super::Server {
    #[tokio::main]
    pub async fn serve_async(
        &self,
        handler: Arc<dyn Send + Sync + Fn(TcpStream) -> BoxFuture<'static, io::Result<()>>>,
    ) -> io::Result<()> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port)).await?;
        println!("listening on: {:?}", listener.local_addr().unwrap());
        let mut task_id = 0;
        let mut join_handles = HashMap::new();

        let (sender, mut receiver) = channel(100);
        loop {
            task_id += 1;
            match listener.accept().await {
                Ok((tcp_stream, socket_addr)) => {
                    let sender = sender.clone();
                    let handler = handler.clone();
                    let join_handle = spawn(async move {
                        println!("handling connection from: {}", socket_addr);
                        let handling_result = (handler)(tcp_stream).await;
                        println!("done handling connection from: {}", socket_addr);
                        sender.send(task_id).await.unwrap();
                        handling_result
                    });
                    join_handles.insert(task_id, join_handle);
                }
                Err(err) => {
                    println!("connection failed: {:?}", err);
                }
            }
            while join_handles.len() >= self.max_connections as usize {
                println!("maximum number of connections reached ({}) - waiting for connections to be closed", self.max_connections);
                let task_id = match receiver.recv().await {
                    Some(task_id) => task_id,
                    None => break,
                };
                if let Some(join_handle) = join_handles.remove(&task_id) {
                    if let Err(error) = join_handle.await? {
                        println!("{error:?}")
                    }
                }
                println!("accepting new connections again");
            }
        }
    }
}
