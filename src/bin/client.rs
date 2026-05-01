use std::io::{ Read, Write };
use std::net::TcpStream;
fn main() {
    let mut stream = TcpStream::connect("127.0.0.1:1234").expect("connect() failed");

    stream.write_all(b"hello").expect("write() failed");

    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).expect("read() failed");

    let msg = std::str::from_utf8(&buf[..n]).unwrap_or("(invalid utf8)");
    println!("server says: {}", msg);

    // close() - automatic as the stream drops
}
