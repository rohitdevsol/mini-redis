use std::{ io::{ Read, Write }, net::{ TcpListener, TcpStream } };

fn do_something(mut stream: TcpStream) {
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).unwrap_or_else(|e| {
        eprintln!("read() error: {}", e);
        0
    });

    if n == 0 {
        return;
    }

    let msg = str::from_utf8(&buf[..n]).unwrap_or("invalid utf8");
    println!("client says: {}", msg);

    stream.write_all(b"ha bhai kya hua").unwrap()
}
fn main() {
    let listener = TcpListener::bind("0.0.0.0:1234").expect("failed to bind");

    println!("Listening on the port 1234");

    for stream in listener.incoming() {
        match stream {
            Ok(conn) => {
                do_something(conn);
            }
            Err(e) => {
                eprintln!("accept() err: {}", e);
                continue;
            }
        }
    }
}
