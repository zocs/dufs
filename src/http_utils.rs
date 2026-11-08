use bytes::{Bytes, BytesMut};
use futures_util::Stream;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Body, Incoming};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::AsyncRead;
use tokio_util::io::poll_read_buf;

#[derive(Debug)]
pub struct IncomingStream {
    inner: Incoming,
}

impl IncomingStream {
    pub fn new(inner: Incoming) -> Self {
        Self { inner }
    }
}

impl Stream for IncomingStream {
    type Item = Result<Bytes, anyhow::Error>;

    #[inline]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match futures_util::ready!(Pin::new(&mut self.inner).poll_frame(cx)?) {
                Some(frame) => match frame.into_data() {
                    Ok(data) => return Poll::Ready(Some(Ok(data))),
                    Err(_frame) => {}
                },
                None => return Poll::Ready(None),
            }
        }
    }
}

pin_project_lite::pin_project! {
    pub struct LengthLimitedStream<R> {
        #[pin]
        reader: Option<R>,
        remaining: u64,
        buf: BytesMut,
        capacity: usize,
    }
}

impl<R> LengthLimitedStream<R> {
    pub fn new(reader: R, limit: u64) -> Self {
        Self {
            reader: Some(reader),
            remaining: limit,
            buf: BytesMut::new(),
            capacity: 4096,
        }
    }
}

impl<R: AsyncRead> Stream for LengthLimitedStream<R> {
    type Item = std::io::Result<Bytes>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();

        if *this.remaining == 0 {
            self.project().reader.set(None);
            return Poll::Ready(None);
        }

        let reader = match this.reader.as_pin_mut() {
            Some(r) => r,
            None => return Poll::Ready(None),
        };

        if this.buf.capacity() == 0 {
            this.buf.reserve(*this.capacity);
        }

        match poll_read_buf(reader, cx, &mut this.buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                self.project().reader.set(None);
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(Ok(0)) => {
                self.project().reader.set(None);
                Poll::Ready(None)
            }
            Poll::Ready(Ok(_)) => {
                let mut chunk = this.buf.split();
                let chunk_size = (*this.remaining).min(chunk.len() as u64) as usize;
                chunk.truncate(chunk_size);
                *this.remaining -= chunk_size as u64;
                Poll::Ready(Some(Ok(chunk.freeze())))
            }
        }
    }
}

pub fn body_full(content: impl Into<hyper::body::Bytes>) -> BoxBody<Bytes, anyhow::Error> {
    Full::new(content.into())
        .map_err(anyhow::Error::new)
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn virtual_range_larger_than_four_gib_keeps_a_64_bit_remaining_count() {
        let limit = u32::MAX as u64 + 65_536;
        let mut stream = LengthLimitedStream::new(tokio::io::repeat(b'x'), limit);
        let chunk = stream.next().await.unwrap().unwrap();
        assert!(!chunk.is_empty() && chunk.len() <= 4096);
        assert_eq!(stream.remaining, limit - chunk.len() as u64);
        assert!(stream.remaining > u32::MAX as u64);
        // Only one bounded chunk is read from the virtual source.
    }

    #[tokio::test]
    async fn a_limited_range_yields_only_its_requested_bytes() {
        let mut stream = LengthLimitedStream::new(&b"0123456789"[..], 3);
        assert_eq!(&stream.next().await.unwrap().unwrap()[..], b"012");
        assert!(stream.next().await.is_none());
        assert!(stream.reader.is_none());
    }
}
