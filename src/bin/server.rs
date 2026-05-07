use libc::{
    EAGAIN,
    // = 11 on Linux/macOS
    // this is what the kernel returns when you try to read/write
    // on a nonblocking fd that isn't ready yet
    // means "try again later, nothing here right now"

    EINTR,
    // = 4
    // means "a system signal interrupted your syscall"
    // has nothing to do with your data, just retry immediately

    F_GETFL,
    F_SETFL,
    O_NONBLOCK,
    POLLERR, // 8  -  "tell me when this fd has an error"
    POLLIN, // 1  -  "tell me when this fd has data to READ"
    POLLOUT, // 4  -  "tell me when this fd is ready to WRITE"
    // these are just integer constants defined in C headers
    // POLLIN as i16 is just casting the integer to the right type
    fcntl,
    poll, // function used for polling
    pollfd, // struct template (type) for a fd on which the poll is working on
};
// pollfd is a STRUCT defined in C as:
// struct pollfd {
//     int   fd;      - which file descriptor to watch
//     short events;  - what you WANT to know about (you set this)
//     short revents; - what ACTUALLY happened (kernel sets this)
// }
// we use it as-is in Rust, same memory layout, same meaning

use std::{
    collections::HashMap,
    io,
    mem,
    net::TcpListener,
    os::unix::io::{ AsRawFd, RawFd },
    sync::Mutex,
};

use mini_redis::hashtable::HMap;

const MAX_MSG: usize = 4096;
const MAX_ARGS: usize = 16;

const SER_NIL: u8 = 0;
const SER_ERR: u8 = 1;
const SER_STR: u8 = 2;
const SER_INT: u8 = 3;
const SER_ARR: u8 = 4;

// error codes
const ERR_UNKNOWN: i32 = 1;
const ERR_2BIG: i32 = 2;

lazy_static::lazy_static! {
    static ref G_MAP: Mutex<HMap> = Mutex::new(HMap::new());
}

#[derive(PartialEq, Clone, Copy)]
enum State {
    Req, //currently reading a request
    Res, //currently writing a response
    End, //done, close this connection
}

// every client will get one of these
struct Conn {
    fd: RawFd,
    state: State,
    rbuf: Vec<u8>, // handles incoming bytes.. grows as the data arrives
    wbuf: Vec<u8>, // holds the response that is wating to be sent

    wbuf_sent: usize, // how many bytes of the wf are already sent
}

impl Conn {
    fn new(fd: RawFd) -> Self {
        Conn {
            fd,
            state: State::Req,
            rbuf: Vec::new(),
            wbuf: Vec::new(),
            wbuf_sent: 0,
        }
    }
}

fn out_nil(out: &mut Vec<u8>) {
    out.push(SER_NIL);
    // that is it. one byte.
}

fn out_str(out: &mut Vec<u8>, val: &str) {
    out.push(SER_STR);
    // 4 byte length
    out.extend_from_slice(&(val.len() as u32).to_le_bytes());
    // actual string bytes
    out.extend_from_slice(val.as_bytes());
}

fn out_int(out: &mut Vec<u8>, val: i64) {
    out.push(SER_INT);
    // 8 bytes for i64
    out.extend_from_slice(&val.to_le_bytes());
}

fn out_err(out: &mut Vec<u8>, code: i32, msg: &str) {
    out.push(SER_ERR);
    // 4 bytes error code
    out.extend_from_slice(&code.to_le_bytes());
    // 4 bytes message length
    out.extend_from_slice(&(msg.len() as u32).to_le_bytes());
    // message bytes
    out.extend_from_slice(msg.as_bytes());
}

fn out_arr(out: &mut Vec<u8>, count: u32) {
    out.push(SER_ARR);
    // just the count — elements are appended separately after this
    out.extend_from_slice(&count.to_le_bytes());
}

// fcntl - file control - used to get or set the properties of an fd
// a general-purpose syscall for getting/setting fd properties
// F_GETFL - get current flags
// F_SETFL - set new flags
// O_NONBLOCK - the flag we want to add (makes io non blocking)
fn fd_set_nb(fd: RawFd) {
    unsafe {
        let flags = fcntl(fd, F_GETFL, 0);
        if flags < 0 {
            eprintln!("fcntl F_GETFL error");
            return;
        }

        // OR the existing flags with O_NONBLOCK to ADD nonblocking
        // we don't want to remove other flags , just add this one
        if fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0 {
            eprintln!("fctnl F_SETFL error");
        }
    }
}

// input:  raw bytes of the request body
// output: Vec<String> of the command parts
// ["set", "key", "value"]  or  ["get", "key"]  or  ["del", "key"]

fn parse_req(data: &[u8]) -> Option<Vec<String>> {
    if data.len() < 4 {
        return None;
    }

    // number of strings
    let nstr = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    if nstr > MAX_ARGS {
        return None;
    }

    let mut cmd: Vec<String> = Vec::new();
    let mut pos = 4;

    for _ in 0..nstr {
        if pos + 4 > data.len() {
            return None;
        }

        let sz = u32::from_le_bytes([
            data[pos],
            data[pos + 1],
            data[pos + 2],
            data[pos + 3],
        ]) as usize;

        pos += 4;

        // need sz more bytes for the string itself
        if pos + sz > data.len() {
            return None;
        }
        let s = String::from_utf8_lossy(&data[pos..pos + sz]).to_string();
        cmd.push(s);
        pos += sz;
    }

    if pos != data.len() {
        return None;
    }

    Some(cmd)
}

fn do_get(cmd: &[String], out: &mut Vec<u8>) {
    let mut map = G_MAP.lock().unwrap();
    match map.get(&cmd[1]) {
        Some(val) => out_str(out, &val.clone()),
        None => out_nil(out),
        // nil instead of RES_NX now — cleaner
    }
}

fn do_del(cmd: &[String], out: &mut Vec<u8>) {
    let mut map = G_MAP.lock().unwrap();
    let deleted = map.del(&cmd[1]);
    // returns integer 1 if deleted, 0 if key was not there
    out_int(out, if deleted { 1 } else { 0 });
}

fn do_set(cmd: &[String], out: &mut Vec<u8>) {
    let mut map = G_MAP.lock().unwrap();
    map.set(cmd[1].clone(), cmd[2].clone());
    out_nil(out); // set always returns nil
}

fn do_keys(out: &mut Vec<u8>) {
    let map = G_MAP.lock().unwrap();
    let keys: Vec<String> = map.all_keys();
    // first write the array header with the count
    out_arr(out, keys.len() as u32);
    // then write each key as a serialized string
    // each one is a full SER_STR value, type byte included
    for key in &keys {
        out_str(out, key);
    }
}
fn do_request(cmd: &[String], out: &mut Vec<u8>) {
    match (cmd[0].to_lowercase().as_str(), cmd.len()) {
        ("keys", 1) => do_keys(out),
        ("get", 2) => do_get(cmd, out),
        ("set", 3) => do_set(cmd, out),
        ("del", 2) => do_del(cmd, out),
        _ => out_err(out, ERR_UNKNOWN, "Unknown cmd"),
    }
}

// look at rbuf - do we have a complete request available ?
// if yes - parse it, generate response into wbuf and remove( drain) it from rbuf
// If no - return false and wait for more data
fn try_one_request(conn: &mut Conn) -> bool {
    // 4 byte for the header
    if conn.rbuf.len() < 4 {
        return false; // it means .. not enough data yet come back later
    }

    let len = u32::from_le_bytes([conn.rbuf[0], conn.rbuf[1], conn.rbuf[2], conn.rbuf[3]]) as usize;

    if len > MAX_MSG {
        eprintln!("message too long");
        conn.state = State::End;
        return false;
    }

    // do we have the full body yet
    if 4 + len > conn.rbuf.len() {
        return false; //not enough data , come back later
    }

    // if we are here then it means we have the complete request
    let req_body = &conn.rbuf[4..4 + len].to_vec();
    let cmd = match parse_req(&req_body) {
        Some(c) => c,
        None => {
            eprintln!("bad request");
            conn.state = State::End;
            return false;
        }
    };

    let mut out: Vec<u8> = Vec::new();
    do_request(&cmd, &mut out);

    // check response is not too big
    if out.len() > MAX_MSG {
        out.clear();
        out_err(&mut out, ERR_2BIG, "response is too big");
    }

    conn.wbuf.clear();
    conn.wbuf_sent = 0;
    conn.wbuf.extend_from_slice(&(out.len() as u32).to_le_bytes());
    conn.wbuf.extend_from_slice(&out);

    // remove this request from the rbuf
    // drain 0..N rmeoves N bytes and shifts the rest to forward
    conn.rbuf.drain(0..4 + len);

    // switching to the response state and send
    conn.state = State::Res;
    try_flush_buffer(conn);

    conn.state == State::Req
}

//read as much data as possible into buffer from the fd into rbuf
// stop when we hit EAGAIN ( no more data right now )
fn try_fill_buffer(conn: &mut Conn) {
    let mut buf = [0u8; 4096];

    loop {
        let rv = unsafe { libc::read(conn.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };

        if rv < 0 {
            let err = io::Error::last_os_error().raw_os_error().unwrap_or(0);

            if err == EINTR {
                // EINTR - interrupted by the a system signal .. we need to retry
                // not our fault and also not a real error
                continue;
            }

            if err == EAGAIN {
                // EAGAIN - no more data right now means we are done for now
                // poll() will tell us when more will arrive
                break;
            }

            eprintln!("read() error");
            conn.state = State::End;
            break;
        }

        if rv == 0 {
            // 0 bytes = means the client closed the connection
            if conn.rbuf.is_empty() {
                eprintln!("EOF");
            } else {
                eprintln!("unexpected EOF");
            }

            conn.state = State::End;
            break;
        }

        // append the data to buffer
        conn.rbuf.extend_from_slice(&buf[..rv as usize]);

        while try_one_request(conn) {}

        if conn.state != State::Req {
            break;
        }
    }
}

fn try_flush_buffer(conn: &mut Conn) {
    loop {
        let remain = &conn.wbuf[conn.wbuf_sent..];

        if remain.is_empty() {
            // everything send .. now we need to go back to reading
            conn.state = State::Req;
            conn.wbuf.clear();
            conn.wbuf_sent = 0;
            break;
        }

        let rv = unsafe {
            libc::write(conn.fd, remain.as_ptr() as *const libc::c_void, remain.len())
        };

        if rv < 0 {
            let err = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err == EINTR {
                continue;
            }
            if err == EAGAIN {
                // kernel write buffer is full right now
                // poll() will notify us when it drains and we can write more
                break;
            }

            eprintln!("write() error");
            conn.state = State::End;
            break;
        }

        conn.wbuf_sent += rv as usize;
        // loop and try to write more
    }
}

// The state machine dispatcher - called when poll() says this fd is ready
fn connection_io(conn: &mut Conn) {
    match conn.state {
        State::Req => try_fill_buffer(conn),
        State::Res => try_flush_buffer(conn),
        State::End => {}
    }
}

fn main() {
    let listener = TcpListener::bind("0.0.0.0:1234").expect("failed to bind");
    let listen_fd = listener.as_raw_fd(); // raw fd number

    fd_set_nb(listen_fd);

    // fd -> Conn map
    let mut fd_to_conn: HashMap<RawFd, Conn> = HashMap::new();

    // poll args - rebuilt every loop iteration
    let mut poll_args: Vec<pollfd> = Vec::new();

    println!("Listening on the port 1234");

    /*
       Think of poll_args as a whiteboard you show the kernel.
        Every loop iteration:
        1. erase the whiteboard        - poll_args.clear()
        2. write current state on it   - push listen_fd, push all active conns
        3. show it to the kernel       - poll(poll_args...)
        4. kernel draws on it          - fills in revents
        5. you read what kernel drew   - check revents
        6. go back to step 1
     */

    loop {
        poll_args.clear();

        poll_args.push(pollfd { fd: listen_fd, events: POLLIN as i16, revents: 0 });

        /* 
            listen_fd with POLLIN means:
            "tell me when a new connection is waiting to be accepted"

            NOT the same as a client fd with POLLIN which means:
            "tell me when this client sent me data"
         */

        for conn in fd_to_conn.values() {
            let events = match conn.state {
                State::Req => POLLIN, // waiting for client to send data
                State::Res => POLLOUT, // waiting for kernel buffer to drain so we can write
                State::End => POLLIN, // placeholder.. will be cleaned up
            };

            poll_args.push(pollfd { fd: conn.fd, events: (events | POLLERR) as i16, revents: 0 });
        }

        // Blocking call
        // only place where server blocks
        // os wakes up when any fd is ready
        // 1000ms timeout
        let rv = unsafe { poll(poll_args.as_mut_ptr(), poll_args.len() as libc::nfds_t, 1000) };

        if rv < 0 {
            eprintln!("poll() error");
            break;
        }

        // collect active fds first to avoid borrow issues
        // index 1 onwards are client connections
        // poll_args[1..]  - all active clients
        let active_fds: Vec<RawFd> = poll_args[1..]
            .iter()
            .filter(|pfd| pfd.revents != 0)
            // revents = what actually happened
            // revents=0 means kernel touched nothing - skip
            // revents≠0 means kernel wrote something - handle it
            .map(|pfd| pfd.fd)
            .collect();

        for fd in active_fds {
            if let Some(conn) = fd_to_conn.get_mut(&fd) {
                connection_io(conn); // real thing happens here
            }

            // clean up connections that are done
            if
                fd_to_conn
                    .get(&fd)
                    .map(|c| c.state == State::End)
                    .unwrap_or(false)
            {
                println!("closing connection fd = {}", fd);
                fd_to_conn.remove(&fd);
                unsafe {
                    libc::close(fd);
                }
            }
        }

        // accept new connections if the listening fd is active
        // poll_args[0] is always the listening fd
        // revents!=0 check for POLLIN "tell me when a new connection is waiting to be accepted"
        if poll_args[0].revents != 0 {
            let conn_fd = unsafe {
                let mut client_addr: libc::sockaddr_in = mem::zeroed();
                let mut socklen = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
                libc::accept(
                    listen_fd,
                    &mut client_addr as *mut _ as *mut libc::sockaddr,
                    &mut socklen
                )
            };

            if conn_fd < 0 {
                eprintln!("accept() error");
            } else {
                fd_set_nb(conn_fd); // CRITICAL - new connections must be nonblocking
                fd_to_conn.insert(conn_fd, Conn::new(conn_fd));
                println!("new connection: fd={}", conn_fd);
            }
        }
    }
}
