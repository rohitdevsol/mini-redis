use std::io::{ ErrorKind, Read, Write };
use std::net::TcpStream;
const MAX_MSG: usize = 4096;

fn query(stream: &mut TcpStream, text: &str) -> Result<(), ()> {
    let text_bytes = text.as_bytes();
    if text_bytes.len() > MAX_MSG {
        eprintln!("message is too long");
        return Err(());
    }

    // building the request

    let len = text_bytes.len() as u32; // header bnega
    let mut send_buf = Vec::new();
    send_buf.extend_from_slice(&len.to_le_bytes()); // header .. length
    send_buf.extend_from_slice(text_bytes); // actual message

    // send this

    stream.write_all(&send_buf).map_err(|_| {
        eprintln!("write() error");
    })?;

    // now we read the reply
    // first 4 bytes
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).map_err(|e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            eprintln!("EOF");
        } else {
            eprintln!("read() error: {}", e);
        }
    })?;

    // get length of the reply
    let reply_len = u32::from_le_bytes(header) as usize;

    if reply_len > MAX_MSG {
        eprintln!("reply too long");
        return Err(());
    }

    // get the actual res from server
    let mut body = vec![0u8;reply_len];

    stream.read_exact(&mut body).map_err(|_| {
        eprintln!("read() error");
    })?;

    // print out the message from server

    println!("server says : {}", String::from_utf8_lossy(&body));
    Ok(())
}
fn main() {
    let mut stream = TcpStream::connect("127.0.0.1:1234").expect("connect() failed");

    query(&mut stream, "hello1").ok();
    query(&mut stream, "hello2").ok();
    query(&mut stream, "hello3").ok();
    // close() - automatic as the stream drops
}
