use tokio::net::TcpStream;

struct Stream {
    tcpstream: TcpStream,
    cipherspec: Vec<u8>,
    bytes_recv: u32,
    bytes_sent: u32,
}

impl Stream {
    pub fn new(tcpstream: TcpStream) -> Self {
        Self {
            tcpstream,
            cipherspec: Vec::new(),
            bytes_recv: 0,
            bytes_sent: 0,
        }
    }
}

mod cipher {
    use std::ops::BitXor;

    #[derive(Clone, Copy)]
    enum Op {
        Reverse,
        Xor(u8),
        XorPos,
        Add(u8),
        AddPos,
        Sub(u8),
        SubPos,
    }

    #[derive(Debug)]
    pub enum Error {
        InvalidOp(u8),
        MissingOperand(usize),
    }

    type Result<T> = std::result::Result<T, Error>;

    pub struct Cipher {
        encoding_ops: Vec<Op>,
        decoding_ops: Vec<Op>,
    }

    impl Cipher {
        pub fn new(spec: &[u8]) -> Result<Self> {
            let mut encoding_ops = Vec::new();
            let mut op_pos = 0;
            while op_pos < spec.len() {
                let op_byte = spec[op_pos];
                let op = match op_byte {
                    0x01 => Op::Reverse,
                    0x02 => Op::Xor(*spec.get(op_pos + 1).ok_or(Error::MissingOperand(op_pos))?),
                    0x03 => Op::XorPos,
                    0x04 => Op::Add(*spec.get(op_pos + 1).ok_or(Error::MissingOperand(op_pos))?),
                    0x05 => Op::AddPos,
                    _ => return Err(Error::InvalidOp(op_byte)),
                };
                match op {
                    Op::Xor(_) | Op::Add(_) => op_pos += 2,
                    _ => op_pos += 1,
                }
                encoding_ops.push(op);
            }
            let decoding_ops = Self::reverse(&encoding_ops);

            Ok(Self {
                encoding_ops,
                decoding_ops,
            })
        }

        pub fn encode(&self, bytes: &mut [u8], stream_pos: usize) {
            for (pos, byte) in bytes.iter_mut().enumerate() {
                *byte = Self::apply(*byte, &self.encoding_ops, pos + stream_pos);
            }
        }

        pub fn decode(&self, bytes: &mut [u8], stream_pos: usize) {
            for (pos, byte) in bytes.iter_mut().enumerate() {
                *byte = Self::apply(*byte, &self.decoding_ops, pos + stream_pos);
            }
        }

        pub fn is_noop(&self) -> bool {
            for byte in 0..255 {
                if Self::apply(byte, &self.encoding_ops, 1)
                    != Self::apply(byte, &self.decoding_ops, 1)
                {
                    return false;
                }
            }
            true
        }

        fn reverse(ops: &[Op]) -> Vec<Op> {
            ops.iter()
                .map(|op| match op {
                    Op::Add(other) => Op::Sub(*other),
                    Op::AddPos => Op::SubPos,
                    other_op => *other_op,
                })
                .rev()
                .collect::<Vec<_>>()
        }

        fn apply(byte: u8, ops: &[Op], stream_pos: usize) -> u8 {
            let mut byte = byte;
            for op in ops.iter() {
                match op {
                    Op::Reverse => byte = byte.reverse_bits(),
                    Op::Xor(other) => byte = byte.bitxor(other),
                    Op::XorPos => byte = byte.bitxor(stream_pos as u8),
                    Op::Add(other) => byte = byte.wrapping_add(*other),
                    Op::AddPos => byte = byte.wrapping_add(stream_pos as u8),
                    Op::Sub(other) => byte = byte.wrapping_sub(*other),
                    Op::SubPos => byte = byte.wrapping_sub(stream_pos as u8),
                }
            }
            byte
        }
    }

    #[cfg(test)]
    mod tests {
        use crate::isl::cipher::Cipher;

        #[test]
        fn encoding_and_decoding() {
            let cipher = Cipher::new(&[0x02, 0x10, 0x01, 0x03, 0x05, 0x04, 0xF1]).unwrap();

            let cleartext = b"10x toy car,15x dog on a string,4x inflatable motorcycle";
            let mut encoded = cleartext.to_owned();

            cipher.encode(&mut encoded[..], 10);

            assert_ne!(cleartext[..], encoded);

            cipher.decode(&mut encoded[..], 10);

            assert_eq!(cleartext[..], encoded);
        }

        #[test]
        fn noop_detection() {
            let valid_cipher = Cipher::new(&[0x02, 0x10, 0x01, 0x03, 0x05, 0x04, 0xF1]).unwrap();

            assert!(!valid_cipher.is_noop());

            let noop_cipher = Cipher::new(&[0x02, 0xa0, 0x02, 0x0b, 0x02, 0xab]).unwrap();

            assert!(noop_cipher.is_noop());
        }
    }
}
