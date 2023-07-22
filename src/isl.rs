mod cipher;

use cipher::Cipher;
use std::io;
use std::io::ErrorKind;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Stream {
    tcpstream: TcpStream,
    cipher: Cipher,
    buf: Vec<u8>,
    bytes_recv: usize,
    bytes_sent: usize,
}

impl Stream {
    pub async fn new(mut tcpstream: TcpStream) -> Result<Self, io::Error> {
        let mut buf = Vec::new();
        let cipher;
        loop {
            tcpstream.read_buf(&mut buf).await?;
            if let Some((end_of_cipher, _)) =
                buf.iter().enumerate().find(|(_, byte)| **byte == 0x00)
            {
                cipher = Cipher::new(buf.drain(..end_of_cipher).as_slice())?;
                buf.drain(..1);
                break;
            }
        }

        if cipher.is_noop() {
            return Err(cipher::Error::Noop.into());
        }

        Ok(Self {
            tcpstream,
            cipher,
            buf,
            bytes_recv: 0,
            bytes_sent: 0,
        })
    }

    pub async fn read_line(&mut self) -> io::Result<String> {
        loop {
            if let Some((end_of_line, _)) = self
                .buf
                .iter()
                .enumerate()
                .find(|(_, byte)| **byte == b'\n')
            {
                let mut line = self.buf.drain(..end_of_line).as_slice().to_vec();
                self.buf.drain(..1);
                self.cipher.decode(&mut line, self.bytes_recv);
                self.bytes_recv += line.len() + 1;
                let decoded_line = String::from_utf8(line.to_vec())
                    .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
                return Ok(decoded_line);
            }
            self.tcpstream.read_buf(&mut self.buf).await?;
        }
    }

    pub async fn write_line(&mut self, line: &str) -> io::Result<()> {
        let mut line = (line.to_owned() + "\n").as_bytes().to_vec();
        self.cipher.encode(&mut line, self.bytes_sent);
        self.bytes_sent += line.len();
        self.tcpstream.write_all(&line).await?;
        Ok(())
    }
}
