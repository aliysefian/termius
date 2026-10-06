//! Bridges for protocols that move a file inside a task that owns the
//! connection (SCP, FTP): the task fills or drains an in-memory pipe, and the
//! pane sees an ordinary reader or writer. What goes wrong in the task comes
//! out of the reader or writer as an error, never as a quiet short file.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

/// Where the task leaves its error, if it has one.
pub type Failure = Arc<Mutex<Option<String>>>;

/// The file's bytes as they arrive. If `expected` is known, ending early is an error.
pub struct PipeReader {
    inner: DuplexStream,
    expected: Option<u64>,
    got: u64,
    failure: Failure,
}

impl PipeReader {
    pub fn new(inner: DuplexStream, expected: Option<u64>, failure: Failure) -> Self {
        Self { inner, expected, got: 0, failure }
    }
}

impl AsyncRead for PipeReader {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let n = (buf.filled().len() - before) as u64;
                self.got += n;
                if n == 0 {
                    let why = self.failure.lock().unwrap_or_else(|p| p.into_inner()).clone();
                    if self.expected.is_some_and(|e| self.got < e) {
                        let why = why.unwrap_or_else(|| "the transfer ended early".into());
                        return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, why)));
                    }
                    // Everything is here, but the closing handshake may still have failed.
                    if let Some(why) = why {
                        return Poll::Ready(Err(std::io::Error::other(why)));
                    }
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

/// Accepts exactly the announced number of bytes; shutting it down waits for the server's verdict.
pub struct PipeWriter {
    inner: DuplexStream,
    size: u64,
    written: u64,
    done: Option<tokio::sync::oneshot::Receiver<Result<(), String>>>,
    closed: bool,
}

impl PipeWriter {
    pub fn new(inner: DuplexStream, size: u64, done: tokio::sync::oneshot::Receiver<Result<(), String>>) -> Self {
        Self { inner, size, written: 0, done: Some(done), closed: false }
    }
}

impl AsyncWrite for PipeWriter {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        if self.written + buf.len() as u64 > self.size {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the file grew while it was being copied")));
        }
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &r {
            self.written += *n as u64;
        }
        r
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if !self.closed {
            match Pin::new(&mut self.inner).poll_shutdown(cx) {
                Poll::Ready(Ok(())) => self.closed = true,
                other => return other,
            }
        }
        let Some(done) = self.done.as_mut() else { return Poll::Ready(Ok(())) };
        match Pin::new(done).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.done = None;
                Poll::Ready(match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(why)) => Err(std::io::Error::other(why)),
                    Err(_) => Err(std::io::Error::other("the transfer was dropped")),
                })
            }
        }
    }
}

use std::future::Future;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn a_reader_ends_in_an_error_when_the_task_gave_up_early() {
        let (mut tx, rx) = tokio::io::duplex(1024);
        let failure: Failure = Arc::default();
        let mut r = PipeReader::new(rx, Some(10), Arc::clone(&failure));
        tx.write_all(b"abc").await.unwrap();
        *failure.lock().unwrap() = Some("the connection dropped".into());
        drop(tx);
        let mut out = Vec::new();
        let e = r.read_to_end(&mut out).await.unwrap_err();
        assert!(e.to_string().contains("the connection dropped"), "{e}");
        assert_eq!(out, b"abc");
    }

    #[tokio::test]
    async fn a_reader_with_everything_ends_cleanly_or_with_the_late_failure() {
        let (mut tx, rx) = tokio::io::duplex(1024);
        let mut r = PipeReader::new(rx, Some(3), Arc::default());
        tx.write_all(b"abc").await.unwrap();
        drop(tx);
        let mut out = Vec::new();
        r.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"abc");
        // The same, but the closing handshake failed.
        let (mut tx, rx) = tokio::io::duplex(1024);
        let failure: Failure = Arc::new(Mutex::new(Some("226 never came".into())));
        let mut r = PipeReader::new(rx, Some(3), failure);
        tx.write_all(b"abc").await.unwrap();
        drop(tx);
        assert!(r.read_to_end(&mut Vec::new()).await.is_err());
        // With no size known, an early end can only be told by the task's error.
        let (tx, rx) = tokio::io::duplex(1024);
        drop(tx);
        let mut r = PipeReader::new(rx, None, Arc::default());
        assert!(r.read_to_end(&mut Vec::new()).await.is_ok());
    }

    #[tokio::test]
    async fn a_writer_refuses_more_than_announced_and_waits_for_the_verdict() {
        let (tx, mut rx) = tokio::io::duplex(1024);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let mut w = PipeWriter::new(tx, 4, done_rx);
        w.write_all(b"abcd").await.unwrap();
        assert!(w.write_all(b"e").await.is_err(), "a fifth byte is more than announced");
        let t = tokio::spawn(async move {
            let mut got = Vec::new();
            rx.read_to_end(&mut got).await.unwrap();
            let _ = done_tx.send(Err("the server said no".to_string()));
            got
        });
        let e = w.shutdown().await.unwrap_err();
        assert!(e.to_string().contains("the server said no"), "{e}");
        assert_eq!(t.await.unwrap(), b"abcd");
    }
}
