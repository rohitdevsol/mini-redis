# Event Loop Flow — Single Request Traced (v2)

## Byte Math First

```
message = "conn0_msg0"
c-o-n-n-0-_-m-s-g-0 = 10 bytes

what travels over the wire:
[10, 0, 0, 0,  c, o, n, n, 0, _, m, s, g, 0]
 ←— 4 bytes —→ ←————————— 10 bytes —————————→
   header=10      actual message

total on wire = 14 bytes

reply = "world" = 5 bytes
reply on wire:
[5, 0, 0, 0,  w, o, r, l, d]
 ←— 4 bytes→ ←— 5 bytes ——→

total reply on wire = 9 bytes
```

---

## Buffer State at Each Step

| Step               | rbuf     | wbuf    |
| ------------------ | -------- | ------- |
| after read()       | 14 bytes | empty   |
| after parse header | 14 bytes | empty   |
| after build reply  | 14 bytes | 9 bytes |
| after drain(0..14) | 0 bytes  | 9 bytes |
| after write()      | 0 bytes  | 0 bytes |

---

## Call Chain (v2 — no middlemen)

```
OLD:  connection_io → state_req → try_fill_buffer → try_one_request → state_res → try_flush_buffer
NEW:  connection_io → try_fill_buffer → try_one_request → try_flush_buffer
```

`state_req` and `state_res` are deleted entirely.
The loops they contained now live inside `try_fill_buffer` and `try_flush_buffer` directly.

---

## Full Flow

```
client connects
→ accept() gives us fd=7
→ Conn { fd=7, state=Req, rbuf=[], wbuf=[] }
→ fd_set_nb(7) → make it nonblocking

next poll() iteration:
  fd=7 is Req state → register with POLLIN
  "wake me when fd=7 has data to read"
  poll() blocks...

client sends "conn0_msg0":
  wire bytes = [10,0,0,0, c,o,n,n,0,_,m,s,g,0]  (14 bytes total)
  kernel buffers these 14 bytes
  kernel wakes poll() → fd=7 revents=POLLIN

connection_io(conn) called
  state=Req → calls try_fill_buffer(conn) directly

try_fill_buffer (self-contained loop):
  rv = read(fd=7, buf, 4096)
  rv = 14 ← positive, got 14 bytes
  rbuf.extend → rbuf = [10,0,0,0,c,o,n,n,0,_,m,s,g,0]  (14 bytes)

  → while try_one_request(conn) {}

      try_one_request:
        rbuf.len() = 14, is 14 >= 4? YES → parse header
        header bytes = [10,0,0,0]
        u32::from_le_bytes → len = 10

        is 4 + 10 <= 14? YES (exactly 14) → we have the full message

        msg = rbuf[4..14] = "conn0_msg0"
        println!("client says: conn0_msg0")

        build reply:
          reply = b"world" = 5 bytes
          reply_len = 5
          wbuf = [5,0,0,0, w,o,r,l,d]  (9 bytes)

        drain rbuf:
          remove rbuf[0..14]  (4 header + 10 body)
          rbuf = []  ← empty now

        state = Res
        calls try_flush_buffer(conn) directly

        try_flush_buffer (self-contained loop):
          remain = wbuf[0..] = 9 bytes
          rv = write(fd=7, wbuf, 9)
          rv = 9 ← kernel accepted all 9 bytes
          wbuf_sent = 9
          wbuf_sent == wbuf.len()? YES → fully sent
          state = Req  ← back to reading
          wbuf = [], wbuf_sent = 0
          break  ← loop ends, returns to try_one_request

        state is now Req
        return conn.state == State::Req → return true
        "finished AND state is back to Req, check for more"

  try_one_request returned true → while loop runs again
    try_one_request:
      rbuf.len() = 0, is 0 >= 4? NO
      return false ← not enough data, while loop ends

  check: conn.state != State::Req? NO → do not break
  loop back to read() at top of try_fill_buffer
    rv = read(fd=7, buf, 4096)
    rv = -1, errno = EAGAIN ← no more data right now
    break ← loop ends, try_fill_buffer returns

back in main loop

next poll() iteration:
  fd=7 still in Req state → register POLLIN again
  poll() blocks waiting for client's next message
```

---

## Diagram

```mermaid
flowchart TD
    A([CLIENT connects]) --> B

    B["accept → fd=7
    Conn {state=Req, rbuf=[], wbuf=[]}
    fd_set_nb fd=7"]

    B --> C

    C[/"poll BLOCKS
    fd=7 registered with POLLIN
    waiting for data..."/]

    C --> D

    D["Client sends wire bytes
    [10,0,0,0, c,o,n,n,0,_,m,s,g,0]
    14 bytes total in kernel buffer"]

    D --> E

    E{{"poll WAKES UP
    fd=7 revents=POLLIN ≠ 0"}}

    E --> F

    F["connection_io called
    state=Req
    → try_fill_buffer directly"]

    F --> G

    G["read fd=7 buf=4096
    rv=14 ✓
    rbuf = [10,0,0,0,c,o,n,n,0,_,m,s,g,0]"]

    G --> H{"rv < 0 ?"}

    H -->|"EINTR"| G
    H -->|"EAGAIN"| BREAK1["break
    no more data right now"]
    H -->|"other error"| ERR1["state=End
    break"]
    H -->|"rv = 0"| EOF["client closed
    state=End, break"]
    H -->|"rv > 0"| I

    I["rbuf.extend 14 bytes
    while try_one_request loop"]

    I --> J{"rbuf.len ≥ 4?"}

    J -->|NO| RET1["return false
    while loop ends"]

    J -->|YES| K

    K["parse header [10,0,0,0]
    len = 10
    need 4+10=14 bytes"]

    K --> L{"4+len ≤ rbuf.len?"}

    L -->|NO| RET2["return false
    partial message, wait"]

    L -->|YES| M

    M["msg = rbuf[4..14] = conn0_msg0
    println client says: conn0_msg0
    build wbuf=[5,0,0,0,w,o,r,l,d]
    drain rbuf[0..14] → rbuf=[]
    state = Res"]

    M --> N["try_flush_buffer directly
    self-contained loop"]

    N --> O{"remain empty?"}

    O -->|YES| P["state=Req
    wbuf=[], wbuf_sent=0
    break"]

    O -->|NO| Q["write fd=7
    rv=9 ← all sent
    wbuf_sent=9"]

    Q --> R{"rv < 0?"}

    R -->|"EINTR"| N
    R -->|"EAGAIN"| BREAK2["break
    poll will wake for POLLOUT"]
    R -->|"other error"| ERR2["state=End
    break"]
    R -->|"rv > 0"| S["wbuf_sent += rv
    loop back"]

    S --> O

    P --> T{"state == Req?
    return true or false"}

    T -->|"true — check for more"| I
    T -->|"false — state changed"| BREAK3["while loop ends"]

    BREAK1 --> U{"state != Req?"}
    BREAK3 --> U

    U -->|YES| V["break out of
    try_fill_buffer loop"]

    U -->|NO| G

    V --> W[/"poll BLOCKS again
    fd=7 registered POLLIN
    waiting for next message"/]

    W -->|"next message arrives"| D

    style A fill:#2d6a4f,color:#fff
    style C fill:#1a1a2e,color:#fff
    style W fill:#1a1a2e,color:#fff
    style E fill:#e76f51,color:#fff
    style M fill:#457b9d,color:#fff
    style P fill:#457b9d,color:#fff
    style ERR1 fill:#6b2737,color:#fff
    style ERR2 fill:#6b2737,color:#fff
    style EOF fill:#6b2737,color:#fff
    style BREAK1 fill:#4a4e69,color:#fff
    style BREAK2 fill:#4a4e69,color:#fff
    style BREAK3 fill:#4a4e69,color:#fff
    style F fill:#f4a261,color:#000
    style N fill:#f4a261,color:#000
```
