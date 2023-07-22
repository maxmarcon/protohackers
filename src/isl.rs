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
    decoded_buf: Vec<u8>,
    bytes_recv: usize,
    bytes_sent: usize,
}

impl Stream {
    pub async fn new(mut tcpstream: TcpStream) -> Result<Self, io::Error> {
        let mut buf = Vec::new();
        let cipher;
        loop {
            tcpstream.read_buf(&mut buf).await?;
            if let Some(end_of_cipher) = Self::find_end_of_cipher(&buf) {
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
            decoded_buf: Vec::new(),
            bytes_recv: 0,
            bytes_sent: 0,
        })
    }

    pub async fn read_line(&mut self) -> io::Result<String> {
        loop {
            if let Some((end_of_line, _)) = self
                .decoded_buf
                .iter()
                .enumerate()
                .find(|(_, byte)| **byte == b'\n')
            {
                let line_bytes = self.decoded_buf.drain(..end_of_line).as_slice().to_vec();
                let line = String::from_utf8(line_bytes.to_vec())
                    .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
                self.decoded_buf.drain(..1);
                println!("decoded: {}", line);
                return Ok(line);
            }
            self.tcpstream.read_buf(&mut self.buf).await?;
            self.cipher.decode(&mut self.buf, self.bytes_recv);
            self.bytes_recv += self.buf.len();
            self.decoded_buf.append(&mut self.buf.drain(..).as_slice().to_vec());
        }
    }

    pub async fn write_line(&mut self, line: &str) -> io::Result<()> {
        println!("sent: {}", line);
        let mut line = (line.to_owned() + "\n").as_bytes().to_vec();
        self.cipher.encode(&mut line, self.bytes_sent);
        self.bytes_sent += line.len();
        self.tcpstream.write_all(&line).await?;
        Ok(())
    }

    fn find_end_of_cipher(bytes: &[u8]) -> Option<usize> {
        for i in 0..bytes.len() {
            if bytes[i] == 0x00 && (i == 0 || (bytes[i - 1] != 0x02 && bytes[i - 1] != 0x04)) {
                return Some(i);
            }
        }
        None
    }
}
