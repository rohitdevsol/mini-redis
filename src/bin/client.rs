use std::io::{ Read, Write };
use std::net::TcpStream;
const MAX_MSG: usize = 4096;

fn on_response(data: &[u8]) -> Result<usize, String> {
    if data.is_empty() {
        return Err("empty response".into());
    }

    match data[0] {
        // NIL
        0 => {
            println!("(nil)");
            Ok(1) // consumed 1 byte
        }

        // ERR: [1 byte type][4 byte code][4 byte msg len][msg]
        1 => {
            if data.len() < 1 + 8 {
                return Err("bad err response".into());
            }
            let code = i32::from_le_bytes([data[1], data[2], data[3], data[4]]);
            let mlen = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
            if data.len() < 1 + 8 + mlen {
                return Err("bad err response".into());
            }
            let msg = String::from_utf8_lossy(&data[9..9 + mlen]);
            println!("(err) {} {}", code, msg);
            Ok(1 + 8 + mlen)
        }

        // STR: [1 byte type][4 byte len][string bytes]
        2 => {
            if data.len() < 1 + 4 {
                return Err("bad str response".into());
            }
            let slen = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
            if data.len() < 1 + 4 + slen {
                return Err("bad str response".into());
            }
            let s = String::from_utf8_lossy(&data[5..5 + slen]);
            println!("(str) {}", s);
            Ok(1 + 4 + slen)
        }

        // INT: [1 byte type][8 byte i64]
        3 => {
            if data.len() < 1 + 8 {
                return Err("bad int response".into());
            }
            let val = i64::from_le_bytes([
                data[1],
                data[2],
                data[3],
                data[4],
                data[5],
                data[6],
                data[7],
                data[8],
            ]);
            println!("(int) {}", val);
            Ok(1 + 8)
        }

        // ARR: [1 byte type][4 byte count][element][element]...
        4 => {
            if data.len() < 1 + 4 {
                return Err("bad arr response".into());
            }
            let count = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
            println!("(arr) len={}", count);

            // each element is a full serialized value
            // we track how many bytes we have consumed
            let mut consumed = 1 + 4;
            for _ in 0..count {
                let rv = on_response(&data[consumed..])?;
                consumed += rv;
            }
            println!("(arr) end");
            Ok(consumed)
        }

        _ => Err("unknown type byte".into()),
    }
}

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

// update read_res to use on_response
fn read_res(stream: &mut TcpStream) -> Result<(), String> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).map_err(|e| e.to_string())?;

    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_MSG {
        return Err("response too long".into());
    }

    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).map_err(|e| e.to_string())?;

    on_response(&body).map(|_| ())
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
