//! High-Performance HTTP Server using may Coroutines
//!
//! This server uses may's stackful coroutines (same as may_minihttp)
//! for maximum performance with minimal memory overhead.
//!
//! Key features:
//! - Stackful coroutines (~2KB stack each vs 8MB for OS threads)
//! - ~10ns context switch (vs ~1µs for OS threads)
//! - Synchronous-looking code with async I/O
//! - Work-stealing scheduler
//!
//! Usage:
//!   cargo run --release --features may-runtime --example http_server_may
//!
//! Benchmark:
//!   wrk -t8 -c400 -d30s -s pipeline.lua http://localhost:8080/

use may::coroutine;
use may::net::{TcpListener, TcpStream};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// Pre-computed HTTP response (zero allocation per request)
const HTTP_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Content-Length: 13\r\n\
Connection: keep-alive\r\n\
\r\n\
Hello, World!";

const BUF_SIZE: usize = 4096;

fn handle_connection(mut stream: TcpStream, count: Arc<AtomicU64>) -> io::Result<()> {
    // Set TCP_NODELAY for low latency
    stream.set_nodelay(true)?;

    let mut read_buf = vec![0u8; BUF_SIZE];
    let mut write_buf = Vec::with_capacity(BUF_SIZE);
    let mut read_pos = 0usize;

    loop {
        // Read data - may's TcpStream automatically yields on WouldBlock
        let n = match stream.read(&mut read_buf[read_pos..]) {
            Ok(0) => return Ok(()), // Connection closed
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // Should not happen with may's blocking I/O, but handle it
                continue;
            }
            Err(e) => return Err(e),
        };
        read_pos += n;

        // Process all complete HTTP requests (pipelining support)
        let mut search_pos = 0;
        while search_pos + 4 <= read_pos {
            // Find \r\n\r\n
            if let Some(pos) = find_header_end(&read_buf[search_pos..read_pos]) {
                // Queue response
                write_buf.extend_from_slice(HTTP_RESPONSE);
                count.fetch_add(1, Ordering::Relaxed);
                search_pos += pos + 4;
            } else {
                break;
            }
        }

        // Write all responses
        if !write_buf.is_empty() {
            stream.write_all(&write_buf)?;
            write_buf.clear();
        }

        // Compact read buffer
        if search_pos > 0 {
            read_buf.copy_within(search_pos..read_pos, 0);
            read_pos -= search_pos;
        }

        // Grow buffer if needed
        if read_pos >= read_buf.len() {
            if read_buf.len() < 65536 {
                read_buf.resize(read_buf.len() * 2, 0);
            }
        }
    }
}

#[inline]
fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn main() -> io::Result<()> {
    // Configure may's coroutine settings
    // Use 6 workers (matching performance cores on M2 Pro)
    let num_workers = 6;

    // Configure may runtime with minimal stack for HTTP handling
    may::config()
        .set_workers(num_workers)
        .set_stack_size(4096); // 4KB stack per coroutine

    println!("==============================================");
    println!("  may Coroutine HTTP Server ({} workers)", num_workers);
    println!("  Stackful Coroutines + Work-Stealing");
    println!("==============================================");
    println!();

    let addr = "0.0.0.0:8080";
    let listener = TcpListener::bind(addr)?;

    println!("Server listening on http://{}", addr);
    println!();
    println!("Benchmark:");
    println!("  wrk -t{} -c400 -d30s -s pipeline.lua http://localhost:8080/", num_workers);
    println!();

    let count = Arc::new(AtomicU64::new(0));

    // Stats coroutine
    let count_clone = count.clone();
    // SAFETY: The spawned coroutine has a valid closure that doesn't violate memory safety
    unsafe {
        coroutine::spawn::<_, ()>(move || {
            let mut last = 0u64;
            let start = std::time::Instant::now();
            loop {
                may::coroutine::sleep(std::time::Duration::from_secs(5));
                let current = count_clone.load(Ordering::Relaxed);
                let rps = (current - last) / 5;
                let elapsed = start.elapsed().as_secs().max(1);
                let avg = current / elapsed;
                println!(
                    "[Stats] Total: {} | Last 5s: {} req/s | Avg: {} req/s",
                    current, rps, avg
                );
                last = current;
            }
        });
    }

    // Accept loop - spawn a coroutine per connection
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let count = count.clone();
                // SAFETY: The spawned coroutine has a valid closure that handles the connection
                unsafe {
                    coroutine::spawn::<_, ()>(move || {
                        if let Err(e) = handle_connection(stream, count) {
                            // Connection errors are expected (client disconnects)
                            let _ = e;
                        }
                    });
                }
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }

    Ok(())
}
