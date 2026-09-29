//! Links carry requests to a device and responses back, independent of the transport.

use std::ops::Range;

use postcard::accumulator::{CobsAccumulator, FeedResult};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::warn;

use red_proto::{MAX_FRAME_LEN, req::Req, resp::Resp};

use crate::Error;

/// A connection to a device, over whichever transport.
///
/// Implementors only move messages; matching responses to requests is the job of the
/// [`Client`](crate::Client).
#[allow(async_fn_in_trait)]
pub trait Link {
    /// Sends a request to the device.
    async fn send(&mut self, req: &Req) -> Result<(), Error>;

    /// Waits for the next response from the device.
    async fn recv(&mut self) -> Result<Resp, Error>;
}

/// Size of each read from the underlying stream.
const READ_LEN: usize = 256;

/// A [`Link`] over a byte stream, delimiting messages with COBS.
///
/// Frames need not line up with reads: one read may carry part of a frame, or several.
pub struct StreamLink<R, W> {
    reader: R,
    writer: W,
    frames: CobsAccumulator<MAX_FRAME_LEN>,
    buf: [u8; READ_LEN],
    /// Bytes in `buf` read from the stream but not yet fed to `frames`
    pending: Range<usize>,
}

impl<R, W> StreamLink<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    /// Creates a link reading responses from `reader` and writing requests to `writer`.
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            frames: CobsAccumulator::new(),
            buf: [0; READ_LEN],
            pending: 0..0,
        }
    }
}

impl<R, W> Link for StreamLink<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    async fn send(&mut self, req: &Req) -> Result<(), Error> {
        let mut frame = [0u8; MAX_FRAME_LEN];
        let n = req.encode(&mut frame)?;

        self.writer.write_all(&frame[..n]).await?;
        self.writer.flush().await?;

        Ok(())
    }

    async fn recv(&mut self) -> Result<Resp, Error> {
        loop {
            if self.pending.is_empty() {
                let n = self.reader.read(&mut self.buf).await?;
                if n == 0 {
                    return Err(Error::Disconnected);
                }
                self.pending = 0..n;
            }

            let end = self.pending.end;
            let window = &self.buf[self.pending.clone()];

            // A bad frame ends at its delimiter, so the accumulator resyncs on the next one
            let rest = match self.frames.feed::<Resp>(window) {
                FeedResult::Consumed => &[][..],
                FeedResult::OverFull(rest) => {
                    warn!("dropped oversized frame");
                    rest
                }
                FeedResult::DeserError(rest) => {
                    warn!("dropped undecodable frame");
                    rest
                }
                FeedResult::Success { data, remaining } => {
                    self.pending = end - remaining.len()..end;
                    return Ok(data);
                }
            };
            self.pending = end - rest.len()..end;
        }
    }
}

#[cfg(test)]
mod tests {
    use red_proto::{
        req::ReqBody,
        resp::{Error as DeviceError, RespBody},
    };
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf, duplex, split};

    use super::*;

    type TestLink = StreamLink<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>;

    /// A link, and the device end of its stream.
    fn link() -> (TestLink, DuplexStream) {
        let (host, device) = duplex(1024);
        let (r, w) = split(host);
        (StreamLink::new(r, w), device)
    }

    fn frame(resp: Resp) -> Vec<u8> {
        let mut buff = [0u8; MAX_FRAME_LEN];
        let n = resp.encode(&mut buff).unwrap();
        buff[..n].to_vec()
    }

    fn failed(id: u32) -> Resp {
        Resp::new(id, RespBody::Error(DeviceError::MeasurementFailed))
    }

    #[tokio::test]
    async fn sends_cobs_frames() {
        let (mut link, mut device) = link();

        link.send(&Req::new(4, ReqBody::GetDeviceInfo))
            .await
            .unwrap();

        let mut buff = [0u8; MAX_FRAME_LEN];
        let n = device.read(&mut buff).await.unwrap();
        let req = Req::decode(&mut buff[..n]).unwrap();
        assert_eq!(req.id, 4);
    }

    #[tokio::test]
    async fn receives_one_frame() {
        let (mut link, mut device) = link();

        device.write_all(&frame(failed(1))).await.unwrap();

        assert_eq!(link.recv().await.unwrap().id, 1);
    }

    #[tokio::test]
    async fn receives_frames_sharing_a_read() {
        let (mut link, mut device) = link();

        let mut both = frame(failed(1));
        both.extend(frame(failed(2)));
        device.write_all(&both).await.unwrap();

        assert_eq!(link.recv().await.unwrap().id, 1);
        assert_eq!(link.recv().await.unwrap().id, 2);
    }

    #[tokio::test]
    async fn receives_a_frame_split_across_reads() {
        let (mut link, mut device) = link();

        let bytes = frame(failed(3));
        let writer = tokio::spawn(async move {
            for b in bytes {
                device.write_all(&[b]).await.unwrap();
                tokio::task::yield_now().await;
            }
            device
        });

        assert_eq!(link.recv().await.unwrap().id, 3);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn resyncs_after_garbage() {
        let (mut link, mut device) = link();

        let mut bytes = vec![0xde, 0xad, 0xbe, 0xef, 0x00];
        bytes.extend(frame(failed(5)));
        device.write_all(&bytes).await.unwrap();

        assert_eq!(link.recv().await.unwrap().id, 5);
    }

    #[tokio::test]
    async fn reports_disconnect() {
        let (mut link, device) = link();

        drop(device);

        assert!(matches!(link.recv().await, Err(Error::Disconnected)));
    }
}
