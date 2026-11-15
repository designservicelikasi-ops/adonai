//! Bounded line framing over an async byte stream.

use crate::MAX_FRAME_BYTES;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Reads newline-terminated frames.
///
/// `next` is cancellation-safe: it may be dropped at any await point (for
/// example inside `tokio::select!`) without losing bytes, because the partial
/// frame lives in the reader, not in the future.
pub struct FrameReader<R> {
    reader: R,
    partial: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> FrameReader<R> {
    pub fn new(reader: R) -> Self {
        FrameReader { reader, partial: Vec::new() }
    }

    /// Next frame, without its newline. `Ok(None)` on a clean end of stream.
    /// A peer that sends more than `MAX_FRAME_BYTES` without a newline gets an
    /// error, so it cannot make the engine buffer unbounded data.
    pub async fn next(&mut self) -> std::io::Result<Option<String>> {
        loop {
            // The only await point. Nothing has been consumed if we are cancelled here.
            let buf = self.reader.fill_buf().await?;
            if buf.is_empty() {
                return if self.partial.is_empty() {
                    Ok(None)
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream ended inside a frame"))
                };
            }
            let (chunk, done) = match buf.iter().position(|b| *b == b'\n') {
                Some(i) => (&buf[..i], Some(i + 1)),
                None => (buf, None),
            };
            if self.partial.len() + chunk.len() > MAX_FRAME_BYTES {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
            }
            self.partial.extend_from_slice(chunk);
            let used = done.unwrap_or(buf.len());
            self.reader.consume(used);
            if done.is_some() {
                let frame = std::mem::take(&mut self.partial);
                return String::from_utf8(frame)
                    .map(Some)
                    .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "frame is not UTF-8"));
            }
        }
    }
}

pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn frames_are_split_on_newlines() {
        let data: &[u8] = b"first\nsecond\n";
        let mut r = FrameReader::new(BufReader::with_capacity(4, data));
        assert_eq!(r.next().await.unwrap().as_deref(), Some("first"));
        assert_eq!(r.next().await.unwrap().as_deref(), Some("second"));
        assert_eq!(r.next().await.unwrap(), None);
    }

    #[tokio::test]
    async fn oversized_and_truncated_frames_are_errors() {
        let big = vec![b'a'; MAX_FRAME_BYTES + 10];
        let mut r = FrameReader::new(BufReader::new(&big[..]));
        assert_eq!(r.next().await.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        let cut: &[u8] = b"no newline";
        let mut r = FrameReader::new(BufReader::new(cut));
        assert_eq!(r.next().await.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
        let bad: &[u8] = &[0xff, 0xfe, b'\n'];
        let mut r = FrameReader::new(BufReader::new(bad));
        assert_eq!(r.next().await.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn write_then_read() {
        let mut out: Vec<u8> = Vec::new();
        write_frame(&mut out, "hello").await.unwrap();
        let mut r = FrameReader::new(BufReader::new(&out[..]));
        assert_eq!(r.next().await.unwrap().as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn a_cancelled_read_does_not_lose_the_beginning_of_a_frame() {
        let (mut tx, rx) = tokio::io::duplex(64);
        let mut r = FrameReader::new(BufReader::new(rx));
        tx.write_all(b"half of a fra").await.unwrap();
        // The read is abandoned while the frame is incomplete (as `select!` does).
        let cancelled = tokio::time::timeout(std::time::Duration::from_millis(30), r.next()).await;
        assert!(cancelled.is_err(), "the frame is not complete yet");
        tx.write_all(b"me\nnext\n").await.unwrap();
        assert_eq!(r.next().await.unwrap().as_deref(), Some("half of a frame"));
        assert_eq!(r.next().await.unwrap().as_deref(), Some("next"));
    }
}
