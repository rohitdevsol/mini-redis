use std::{ io::{ BufReader, ErrorKind, Read, Write }, net::{ TcpListener, TcpStream } };

const MAX_MSG: usize = 4096;
fn one_request(reader: &mut BufReader<TcpStream>, stream: &mut TcpStream) -> Result<(), ()> {
    // 1. read exac 4 bytes
    let mut header = [0u8; 4];
    reader.read_exact(&mut header).map_err(|e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            eprintln!("EOF");
        } else {
            eprintln!("read() error: {}", e);
        }
    })?;

    // 2. now we can parse the remaining
    let len = u32::from_le_bytes(header) as usize; // parsing the length here ..  ex 6 0 0 0 becomes 6

    if len > MAX_MSG {
        eprintln!("too long");
        return Err(());
    }

    //3. read exac len bytes
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|_| {
        eprintln!("read() error");
    })?;

    // 4. print msg
    let msg = String::from_utf8_lossy(&body);
    println!("client says: {}", msg);

    let reply = b"world";
    let reply_len = reply.len() as u32;

    //5. send a reply
    // we will also attach the length in begining
    let mut reply_buf = Vec::new();
    reply_buf.extend_from_slice(&reply_len.to_le_bytes()); // header
    reply_buf.extend_from_slice(reply);

    stream.write_all(&reply_buf).map_err(|_| { eprintln!("write() error") })?;
    Ok(())
}

// fn do_something(mut stream: TcpStream) {
//     let mut buf = [0u8; 64];
//     let n = stream.read(&mut buf).unwrap_or_else(|e| {
//         eprintln!("read() error: {}", e);
//         0
//     });

//     if n == 0 {
//         return;
//     }

//     let msg = str::from_utf8(&buf[..n]).unwrap_or("invalid utf8");
//     println!("client says: {}", msg);

//     stream.write_all(b"ha bhai kya hua").unwrap()
// }
fn main() {
    let listener = TcpListener::bind("0.0.0.0:1234").expect("failed to bind");

    println!("Listening on the port 1234");

    for res in listener.incoming() {
        match res {
            Ok(mut conn) => {
                //internally holds a buffer - default is 8kb
                //read big chunks from the kernel and then can server reads from memory
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                loop {
                    if one_request(&mut reader, &mut conn).is_err() {
                        break;
                    }
                }
                // close() automatic when dropped
            }
            Err(e) => {
                eprintln!("accept() err: {}", e);
                continue;
            }
        }
    }
}
