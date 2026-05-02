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

use std::{ collections::HashMap, io, mem, net::TcpListener, os::unix::io::{ AsRawFd, RawFd } };

const MAX_MSG: usize = 4096;

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
    let msg = String::from_utf8_lossy(&conn.rbuf[4..4 + len]);
    println!("client says: {}", msg);

    let reply = b"world";
    let reply_len = reply.len() as u32;
    conn.wbuf.clear();
    conn.wbuf_sent = 0;
    conn.wbuf.extend_from_slice(&reply_len.to_le_bytes());
    conn.wbuf.extend_from_slice(reply);

    // remove this request from the rbuf
    // drain 0..N rmeoves N bytes and shifts the rest to forward
    conn.rbuf.drain(0..4 + len);

    // switching to the response state and send
    conn.state = State::Res;
    state_res(conn); // try_flush_buffer

    conn.state == State::Req
}

//read as much data as possible into buffer from the fd into rbuf
// stop when we hit EAGAIN ( no more data right now )
fn try_fill_buffer(conn: &mut Conn) -> bool {
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
                return false;
            }

            eprintln!("read() error");
            conn.state = State::End;
            return false;
        }

        if rv == 0 {
            // 0 bytes = means the client closed the connection
            if conn.rbuf.is_empty() {
                eprintln!("EOF");
            } else {
                eprintln!("unexpected EOF");
            }

            conn.state = State::End;
            return false;
        }

        // append the data to buffer
        conn.rbuf.extend_from_slice(&buf[..rv as usize]);

        while try_one_request(conn) {}

        return conn.state == State::Req;
    }
}

fn state_req(conn: &mut Conn) {
    // loop try_fill_buffer until it returns false (EAGAIN or state changed)
    while try_fill_buffer(conn) {}
    // keep filling as long as there is work to do
    // stops when EAGAIN or state changes
}

fn try_flush_buffer(conn: &mut Conn) -> bool {
    loop {
        let remain = &conn.wbuf[conn.wbuf_sent..];

        if remain.is_empty() {
            // everything send .. now we need to go back to reading
            conn.state = State::Req;
            conn.wbuf.clear();
            conn.wbuf_sent = 0;
            return false;
        }

        let rv = unsafe {
            libc::write(conn.fd, remain.as_ptr() as *const libc::c_void, remain.len())
        };

        if rv < 0 {
            let err = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err == EAGAIN {
                // kernel write buffer is full right now
                // poll() will notify us when it drains and we can write more
                return false;
            }

            eprintln!("write() error");
            conn.state = State::End;
            return false;
        }

        conn.wbuf_sent += rv as usize;
        // loop and try to write more
    }
}

fn state_res(conn: &mut Conn) {
    while try_flush_buffer(conn) {}
    // keep flushing as long as there is data to send
    // stops when EAGAIN or everything sent
}

// The state machine dispatcher - called when poll() says this fd is ready
fn connection_io(conn: &mut Conn) {
    match conn.state {
        State::Req => state_req(conn),
        State::Res => state_res(conn),
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
