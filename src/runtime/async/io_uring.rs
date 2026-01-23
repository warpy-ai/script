//! io_uring backend for Linux
//!
//! Provides high-performance async I/O using Linux 5.1+ io_uring.
//! This module is only available on Linux with the `io-uring` feature enabled.

use std::collections::HashMap;
use std::io;
use std::os::unix::io::RawFd;
use std::task::Waker;

use io_uring::{opcode, types, IoUring};

use super::{Interest, Token};

/// Ring buffer size (number of entries)
const RING_SIZE: u32 = 256;

/// io_uring-based reactor for high-performance async I/O
pub struct IoUringReactor {
    /// The io_uring instance
    ring: IoUring,
    /// Map from user_data token to (fd, waker)
    pending: HashMap<u64, PendingOp>,
    /// Next token to assign
    next_token: u64,
}

/// A pending I/O operation
struct PendingOp {
    fd: RawFd,
    waker: Option<Waker>,
    op_type: OpType,
}

/// Type of operation for result interpretation
#[derive(Clone, Copy, Debug)]
enum OpType {
    Poll(Interest),
    Read,
    Write,
    Accept,
    Connect,
}

impl IoUringReactor {
    /// Create a new io_uring reactor
    pub fn new() -> io::Result<Self> {
        let ring = IoUring::new(RING_SIZE)?;
        Ok(Self {
            ring,
            pending: HashMap::new(),
            next_token: 0,
        })
    }

    /// Allocate a new token for tracking operations
    fn alloc_token(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1);
        token
    }

    /// Register interest in a file descriptor (poll-based)
    pub fn register(
        &mut self,
        fd: RawFd,
        interest: Interest,
        waker: Option<Waker>,
    ) -> io::Result<Token> {
        let token = self.alloc_token();

        let poll_flags = match interest {
            Interest::Readable => libc::POLLIN as u32,
            Interest::Writable => libc::POLLOUT as u32,
        };

        let entry = opcode::PollAdd::new(types::Fd(fd), poll_flags)
            .build()
            .user_data(token);

        // Safety: We own the ring and entry is valid
        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "submission queue full"))?;
        }

        self.pending.insert(
            token,
            PendingOp {
                fd,
                waker,
                op_type: OpType::Poll(interest),
            },
        );

        Ok(Token(token as usize))
    }

    /// Submit pending operations to the kernel
    pub fn submit(&mut self) -> io::Result<usize> {
        self.ring.submit().map_err(|e| e.into())
    }

    /// Submit and wait for at least one completion
    pub fn submit_and_wait(&mut self, min_complete: usize) -> io::Result<usize> {
        self.ring.submit_and_wait(min_complete).map_err(|e| e.into())
    }

    /// Wait for I/O events with optional timeout
    pub fn wait(&mut self, timeout_ms: i32) -> io::Result<Vec<(Token, Interest)>> {
        // Submit any pending entries
        self.ring.submit()?;

        // If timeout is specified, we need to handle it
        // For now, do a simple poll
        if timeout_ms == 0 {
            // Non-blocking poll
        } else if timeout_ms > 0 {
            // Wait with timeout - submit_and_wait with at least 1
            let _ = self.ring.submit_and_wait(1);
        } else {
            // Wait indefinitely
            self.ring.submit_and_wait(1)?;
        }

        self.collect_completions()
    }

    /// Collect completed operations from the completion queue
    fn collect_completions(&mut self) -> io::Result<Vec<(Token, Interest)>> {
        let mut results = Vec::new();

        // Drain the completion queue
        for cqe in self.ring.completion() {
            let token = cqe.user_data();
            let result = cqe.result();

            if let Some(pending) = self.pending.remove(&token) {
                // Wake the associated waker if present
                if let Some(waker) = pending.waker {
                    waker.wake();
                }

                // Determine the interest based on operation type
                let interest = match pending.op_type {
                    OpType::Poll(i) => i,
                    OpType::Read => Interest::Readable,
                    OpType::Write => Interest::Writable,
                    OpType::Accept => Interest::Readable,
                    OpType::Connect => Interest::Writable,
                };

                // Only report successful completions
                if result >= 0 {
                    results.push((Token(token as usize), interest));
                }
            }
        }

        Ok(results)
    }

    /// Cancel a pending operation
    pub fn cancel(&mut self, token: Token) -> io::Result<()> {
        let cancel_token = self.alloc_token();

        let entry = opcode::AsyncCancel::new(token.0 as u64)
            .build()
            .user_data(cancel_token);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "submission queue full"))?;
        }

        self.pending.remove(&(token.0 as u64));
        Ok(())
    }

    /// Deregister a file descriptor (remove from pending)
    pub fn deregister(&mut self, token: Token) {
        self.pending.remove(&(token.0 as u64));
    }
}

// Extended operations using io_uring's native async capabilities

impl IoUringReactor {
    /// Submit an async read operation
    pub fn submit_read(
        &mut self,
        fd: RawFd,
        buf: *mut u8,
        len: u32,
        offset: u64,
        waker: Option<Waker>,
    ) -> io::Result<Token> {
        let token = self.alloc_token();

        let entry = opcode::Read::new(types::Fd(fd), buf, len)
            .offset(offset)
            .build()
            .user_data(token);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "submission queue full"))?;
        }

        self.pending.insert(
            token,
            PendingOp {
                fd,
                waker,
                op_type: OpType::Read,
            },
        );

        Ok(Token(token as usize))
    }

    /// Submit an async write operation
    pub fn submit_write(
        &mut self,
        fd: RawFd,
        buf: *const u8,
        len: u32,
        offset: u64,
        waker: Option<Waker>,
    ) -> io::Result<Token> {
        let token = self.alloc_token();

        let entry = opcode::Write::new(types::Fd(fd), buf, len)
            .offset(offset)
            .build()
            .user_data(token);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "submission queue full"))?;
        }

        self.pending.insert(
            token,
            PendingOp {
                fd,
                waker,
                op_type: OpType::Write,
            },
        );

        Ok(Token(token as usize))
    }

    /// Submit an async accept operation
    pub fn submit_accept(&mut self, fd: RawFd, waker: Option<Waker>) -> io::Result<Token> {
        let token = self.alloc_token();

        let entry = opcode::Accept::new(types::Fd(fd), std::ptr::null_mut(), std::ptr::null_mut())
            .build()
            .user_data(token);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "submission queue full"))?;
        }

        self.pending.insert(
            token,
            PendingOp {
                fd,
                waker,
                op_type: OpType::Accept,
            },
        );

        Ok(Token(token as usize))
    }

    /// Submit an async connect operation
    pub fn submit_connect(
        &mut self,
        fd: RawFd,
        addr: *const libc::sockaddr,
        addrlen: libc::socklen_t,
        waker: Option<Waker>,
    ) -> io::Result<Token> {
        let token = self.alloc_token();

        let entry = opcode::Connect::new(types::Fd(fd), addr, addrlen)
            .build()
            .user_data(token);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "submission queue full"))?;
        }

        self.pending.insert(
            token,
            PendingOp {
                fd,
                waker,
                op_type: OpType::Connect,
            },
        );

        Ok(Token(token as usize))
    }

    /// Get the result of a completed operation
    /// Returns (bytes_transferred, error_code) where error_code is 0 on success
    pub fn get_result(&self, token: Token) -> Option<i32> {
        // Results are consumed during collect_completions
        // This is a placeholder for future result caching
        None
    }
}

impl Drop for IoUringReactor {
    fn drop(&mut self) {
        // Cancel all pending operations
        for (token, _) in self.pending.drain() {
            let _ = self.cancel(Token(token as usize));
        }
        // Submit cancellations
        let _ = self.ring.submit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reactor_creation() {
        let reactor = IoUringReactor::new();
        assert!(reactor.is_ok(), "Failed to create io_uring reactor");
    }

    #[test]
    fn test_token_allocation() {
        let mut reactor = IoUringReactor::new().unwrap();
        let t1 = reactor.alloc_token();
        let t2 = reactor.alloc_token();
        assert_ne!(t1, t2, "Tokens should be unique");
    }
}
