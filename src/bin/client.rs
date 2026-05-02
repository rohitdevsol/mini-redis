use std::io::{ ErrorKind, Read, Write };
use std::net::TcpStream;
const MAX_MSG: usize = 4096;

fn send_request(stream: &mut TcpStream, text: &str) -> Result<(), ()> {
    let bytes = text.as_bytes();

    if bytes.len() > MAX_MSG {
        return Err(());
    }

    let mut wbuf = Vec::new();
    wbuf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    wbuf.extend_from_slice(bytes);

    stream.write_all(&wbuf).map_err(|_| eprintln!("write error"))?;
    Ok(())
}

fn read_res(stream: &mut TcpStream) -> Result<(), ()> {
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

    let queries = ["hello1", "hello2", "hello3"];
    for q in &queries {
        if send_request(&mut stream, q).is_err() {
            return;
        }
    }

    for _ in &queries {
        if read_res(&mut stream).is_err() {
            return;
        }
    }
}
