use std::io::{ ErrorKind, Read, Write };
use std::net::TcpStream;
const MAX_MSG: usize = 4096;

fn send_req(stream: &mut TcpStream, cmd: &[&str]) -> Result<(), String> {
    // calculate total body size:
    // 4 bytes for nstr + for each string: 4 bytes length + string bytes
    let body_len: usize =
        4 +
        cmd
            .iter()
            .map(|s| 4 + s.len())
            .sum::<usize>();

    if body_len > MAX_MSG {
        return Err("command too long".into());
    }

    let mut wbuf: Vec<u8> = Vec::new();

    // outer frame length
    wbuf.extend_from_slice(&(body_len as u32).to_le_bytes());

    // number of strings
    wbuf.extend_from_slice(&(cmd.len() as u32).to_le_bytes());

    // each string: [4 byte length][bytes]
    for s in cmd {
        wbuf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        wbuf.extend_from_slice(s.as_bytes());
    }

    stream.write_all(&wbuf).map_err(|e| format!("write error: {}", e))
}

// wire format: [header 4B][rescode 4B][data...]
fn read_res(stream: &mut TcpStream) -> Result<(), String> {
    // read outer length header
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).map_err(|e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            "EOF".to_string()
        } else {
            format!("read error: {}", e)
        }
    })?;

    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_MSG {
        return Err("response too long".into());
    }

    // read the full body: rescode (4 bytes) + data
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).map_err(|e| format!("read error: {}", e))?;

    // first 4 bytes of body = rescode
    if body.len() < 4 {
        return Err("bad response".into());
    }

    let rescode = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);

    // rest is the actual data
    let data = String::from_utf8_lossy(&body[4..]);

    // mirrors the C printf:  server says: [rescode] data
    println!("server says: [{}] {}", rescode, data);

    Ok(())
}
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: client <command> [args...]");
        eprintln!("  client get key");
        eprintln!("  client set key value");
        eprintln!("  client del key");
        std::process::exit(1);
    }

    // args[0] is binary name, args[1..] is the command
    let cmd: Vec<&str> = args[1..]
        .iter()
        .map(|s| s.as_str())
        .collect();

    let mut stream = TcpStream::connect("127.0.0.1:1234").unwrap_or_else(|_| {
        eprintln!("connect() failed");
        std::process::exit(1);
    });

    send_req(&mut stream, &cmd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    read_res(&mut stream).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });
}
