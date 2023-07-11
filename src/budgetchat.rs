use std::io;
use std::io::ErrorKind;

pub fn parse_message(buffer: &mut Vec<u8>) -> io::Result<Option<String>> {
    if let Some(pos) = buffer
        .iter()
        .enumerate()
        .find(|(_pos, c)| **c == b'\n')
        .map(|(pos, _)| pos)
    {
        let message = String::from_utf8(buffer.drain(..pos).collect())
            .map(Some)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e));

        buffer.remove(0);

        return message;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::parse_message;
    use std::io;

    #[test]
    fn test_parse_single_message() {
        let parsed = parse_message(&mut b"hello world\n".to_vec());
        check_parsed_message(parsed, "hello world");

        let parsed = parse_message(&mut b"hello world\nhow are you?".to_vec());
        check_parsed_message(parsed, "hello world");

        let parsed = parse_message(&mut b"hello world".to_vec());
        assert!(parsed.is_ok());
        assert!(parsed.unwrap().is_none());

        let parsed = parse_message(&mut b"\n".to_vec());
        check_parsed_message(parsed, "");

        let parsed = parse_message(&mut b" hello \n".to_vec());
        check_parsed_message(parsed, " hello ");
    }

    #[test]
    fn test_parse_multiple_messages() {
        let mut buffer = b"hello world\nhow are you?\n".to_vec();

        let parsed = parse_message(&mut buffer);
        check_parsed_message(parsed, "hello world");

        let parsed = parse_message(&mut buffer);
        check_parsed_message(parsed, "how are you?");
    }

    fn check_parsed_message(parsed: io::Result<Option<String>>, msg: &str) {
        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap(), msg);
    }
}
