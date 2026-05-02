use std::io::{ Read, Write, ErrorKind };
use std::net::TcpStream;
use std::thread;
use std::time::{ Duration, Instant };
use std::sync::{ Arc, Mutex };

const K_MAX_MSG: usize = 4096;
const NUM_CONNECTIONS: usize = 10;
const MSGS_PER_CONNECTION: usize = 10;

// ─── Stats per connection ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ConnStats {
    conn_id: usize,
    messages_sent: usize,
    messages_received: usize,
    failed: bool,
    duration_ms: u128,
    // individual message round trip times
    msg_times_us: Vec<u128>, // microseconds per message
}

// ─── Protocol helpers ─────────────────────────────────────────────────────────

fn send_req(stream: &mut TcpStream, text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    if bytes.len() > K_MAX_MSG {
        return Err("message too long".into());
    }

    let mut wbuf = Vec::new();
    wbuf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    wbuf.extend_from_slice(bytes);

    stream.write_all(&wbuf).map_err(|e| format!("write error: {}", e))
}

fn read_res(stream: &mut TcpStream) -> Result<String, String> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).map_err(|e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            "EOF".to_string()
        } else {
            format!("read header error: {}", e)
        }
    })?;

    let len = u32::from_le_bytes(header) as usize;
    if len > K_MAX_MSG {
        return Err("reply too long".into());
    }

    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).map_err(|e| format!("read body error: {}", e))?;

    Ok(String::from_utf8_lossy(&body).to_string())
}

// ─── Single connection worker ─────────────────────────────────────────────────
// This runs in its own thread
// Sends 10 messages using PIPELINING
// Records timing for each message

fn run_connection(conn_id: usize) -> ConnStats {
    let mut stats = ConnStats {
        conn_id,
        messages_sent: 0,
        messages_received: 0,
        failed: false,
        duration_ms: 0,
        msg_times_us: Vec::new(),
    };

    let conn_start = Instant::now();

    // connect
    let mut stream = match TcpStream::connect("127.0.0.1:1234") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[conn {}] connect failed: {}", conn_id, e);
            stats.failed = true;
            return stats;
        }
    };

    // set a read timeout so we don't hang forever if server dies
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    // build messages for this connection
    // each message includes conn_id and message number so we can trace them
    let messages: Vec<String> = (0..MSGS_PER_CONNECTION)
        .map(|i| format!("conn{}_msg{}", conn_id, i))
        .collect();

    // ── PIPELINE: send ALL requests first ─────────────────────────────────────
    let send_start = Instant::now();
    for msg in &messages {
        match send_req(&mut stream, msg) {
            Ok(_) => {
                stats.messages_sent += 1;
            }
            Err(e) => {
                eprintln!("[conn {}] send error: {}", conn_id, e);
                stats.failed = true;
                stats.duration_ms = conn_start.elapsed().as_millis();
                return stats;
            }
        }
    }
    let send_done = send_start.elapsed().as_micros();

    // ── PIPELINE: read ALL responses ──────────────────────────────────────────
    for i in 0..MSGS_PER_CONNECTION {
        let msg_start = Instant::now();

        match read_res(&mut stream) {
            Ok(reply) => {
                let elapsed = msg_start.elapsed().as_micros();
                stats.msg_times_us.push(elapsed);
                stats.messages_received += 1;

                // verify reply is correct
                if reply != "world" {
                    eprintln!("[conn {}] msg {} got unexpected reply: {}", conn_id, i, reply);
                }
            }
            Err(e) => {
                eprintln!("[conn {}] read error on msg {}: {}", conn_id, i, e);
                stats.failed = true;
                break;
            }
        }
    }

    stats.duration_ms = conn_start.elapsed().as_millis();

    println!(
        "[conn {:>2}] ✓ sent {} | received {} | send_phase={}µs | total={}ms",
        conn_id,
        stats.messages_sent,
        stats.messages_received,
        send_done,
        stats.duration_ms
    );

    stats
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "  Test: {} connections × {} messages = {} total",
        NUM_CONNECTIONS,
        MSGS_PER_CONNECTION,
        NUM_CONNECTIONS * MSGS_PER_CONNECTION
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let all_stats: Arc<Mutex<Vec<ConnStats>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    let overall_start = Instant::now();

    // spawn all 10 connection threads simultaneously
    // this is the real test — server handles them all at the same time
    for conn_id in 0..NUM_CONNECTIONS {
        let stats_ref = Arc::clone(&all_stats);

        let handle = thread::spawn(move || {
            let stats = run_connection(conn_id);
            stats_ref.lock().unwrap().push(stats);
        });

        handles.push(handle);
    }

    // wait for all threads to finish
    for handle in handles {
        handle.join().unwrap();
    }

    let overall_elapsed = overall_start.elapsed();

    // ── Print Summary ─────────────────────────────────────────────────────────
    let stats = all_stats.lock().unwrap();
    let mut all_msg_times: Vec<u128> = Vec::new();

    let mut total_sent = 0;
    let mut total_received = 0;
    let mut failed_conns = 0;
    let mut conn_durations: Vec<u128> = Vec::new();

    for s in stats.iter() {
        total_sent += s.messages_sent;
        total_received += s.messages_received;
        if s.failed {
            failed_conns += 1;
        }
        conn_durations.push(s.duration_ms);
        all_msg_times.extend_from_slice(&s.msg_times_us);
    }

    // sort for percentile calculations
    all_msg_times.sort_unstable();
    conn_durations.sort_unstable();

    let avg_msg_us = if !all_msg_times.is_empty() {
        all_msg_times.iter().sum::<u128>() / (all_msg_times.len() as u128)
    } else {
        0
    };

    let p50 = percentile(&all_msg_times, 50);
    let p95 = percentile(&all_msg_times, 95);
    let p99 = percentile(&all_msg_times, 99);

    let throughput = if overall_elapsed.as_secs_f64() > 0.0 {
        (total_received as f64) / overall_elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  RESULTS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Connections       : {}", NUM_CONNECTIONS);
    println!("  Failed conns      : {}", failed_conns);
    println!("  Total sent        : {}", total_sent);
    println!("  Total received    : {}", total_received);
    println!("  Overall time      : {:.2}ms", overall_elapsed.as_secs_f64() * 1000.0);
    println!("  Throughput        : {:.0} msgs/sec", throughput);
    println!();
    println!("  ── Per-message response time (µs) ──");
    println!("  Min               : {}µs", all_msg_times.first().unwrap_or(&0));
    println!("  Avg               : {}µs", avg_msg_us);
    println!("  p50 (median)      : {}µs", p50);
    println!("  p95               : {}µs", p95);
    println!("  p99               : {}µs", p99);
    println!("  Max               : {}µs", all_msg_times.last().unwrap_or(&0));
    println!();
    println!("  ── Per-connection total time ────────");
    println!("  Fastest conn      : {}ms", conn_durations.first().unwrap_or(&0));
    println!("  Slowest conn      : {}ms", conn_durations.last().unwrap_or(&0));
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

fn percentile(sorted: &[u128], pct: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() * pct) / 100).min(sorted.len() - 1);
    sorted[idx]
}
