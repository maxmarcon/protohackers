mod cipher;

use cipher::Cipher;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Stream {
    tcpstream: TcpStream,
    cipher: Cipher,
    initially_buffered: Vec<u8>,
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

        cipher.decode(&mut buf, 0);
        let initially_buffered_len = buf.len();

        Ok(Self {
            tcpstream,
            cipher,
            initially_buffered: buf,
            bytes_recv: initially_buffered_len,
            bytes_sent: 0,
        })
    }

    pub async fn read_buf(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        if !self.initially_buffered.is_empty() {
            let buffered = self.initially_buffered.len();
            buf.append(&mut self.initially_buffered);
            return Ok(buffered);
        }
        let to_decode_start = buf.len();
        let read = self.tcpstream.read_buf(buf).await?;
        if read > 0 {
            self.cipher
                .decode(&mut buf[to_decode_start..], self.bytes_recv);
            self.bytes_recv += read;
        }
        Ok(read)
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let mut encode_buf = buf.to_owned();
        self.cipher.encode(&mut encode_buf, self.bytes_sent);
        self.tcpstream.write_all(&encode_buf).await?;
        self.bytes_sent += encode_buf.len();
        Ok(())
    }

    fn find_end_of_cipher(bytes: &[u8]) -> Option<usize> {
        (0..bytes.len()).find(|&i| {
            bytes[i] == 0x00 && (i == 0 || (bytes[i - 1] != 0x02 && bytes[i - 1] != 0x04))
        })
    }
}
