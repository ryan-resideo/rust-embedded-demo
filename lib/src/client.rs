//! Request/response exchange with a device over any [`Link`].

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::debug;

use red_proto::{
    req::{Req, ReqBody},
    resp::{DeviceInfo, Measurement, RespBody},
};

use crate::{Error, Link};

/// Issues requests to a device and matches up the responses.
pub struct Client<L> {
    link: L,
    next_id: u32,
    timeout: Duration,
}

impl<L: Link> Client<L> {
    /// How long [`Client::request`] waits for a response by default.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

    /// Creates a client over the given link.
    pub fn new(link: L) -> Self {
        // Start somewhere arbitrary, so a late reply to a previous client's request is not
        // mistaken for a reply to this one's.
        let next_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or_default();

        Self {
            link,
            next_id,
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }

    /// Sets how long to wait for each response.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sends a request and waits for the response carrying its identifier.
    ///
    /// Responses to other requests, such as a late reply to one that timed out, are
    /// discarded.
    pub async fn request(&mut self, body: ReqBody) -> Result<RespBody, Error> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        self.link.send(&Req::new(id, body)).await?;

        let link = &mut self.link;
        let wait = async {
            loop {
                let resp = link.recv().await?;
                if resp.id == id {
                    return Ok(resp.body);
                }
                debug!("discarding response {} while waiting for {}", resp.id, id);
            }
        };

        tokio::time::timeout(self.timeout, wait)
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Fetches the device's identity and firmware version.
    pub async fn device_info(&mut self) -> Result<DeviceInfo, Error> {
        match self.request(ReqBody::GetDeviceInfo).await? {
            RespBody::DeviceInfo(info) => Ok(info),
            RespBody::Error(e) => Err(Error::Device(e)),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    /// Fetches the device's latest measurement.
    pub async fn measurement(&mut self) -> Result<Measurement, Error> {
        match self.request(ReqBody::GetMeasurement).await? {
            RespBody::Measurement(measurement) => Ok(measurement),
            RespBody::Error(e) => Err(Error::Device(e)),
            _ => Err(Error::UnexpectedResponse),
        }
    }
}

#[cfg(test)]
mod tests {
    use heapless::String;
    use postcard::accumulator::{CobsAccumulator, FeedResult};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex, split};

    use red_proto::{
        MAX_FRAME_LEN,
        resp::{Error as DeviceError, Resp},
    };

    use super::*;
    use crate::StreamLink;

    const SAMPLE: Measurement = Measurement {
        temperature: 21.0,
        pressure: 101_325.0,
        humidity: 55.0,
    };

    /// Runs a fake device on `stream`, answering each request with whatever `reply` returns.
    async fn device(stream: DuplexStream, reply: impl Fn(Req) -> Vec<Resp>) {
        let (mut r, mut w) = split(stream);
        let mut frames = CobsAccumulator::<MAX_FRAME_LEN>::new();
        let mut buf = [0u8; 64];

        loop {
            let n = match r.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let mut window = &buf[..n];
            while let FeedResult::Success { data, remaining } = frames.feed::<Req>(window) {
                for resp in reply(data) {
                    let mut out = [0u8; MAX_FRAME_LEN];
                    let n = resp.encode(&mut out).unwrap();
                    w.write_all(&out[..n]).await.unwrap();
                }
                window = remaining;
            }
        }
    }

    /// A client connected to a fake device answering with `reply`.
    fn client(reply: impl Fn(Req) -> Vec<Resp> + Send + 'static) -> Client<impl Link> {
        let (host, dev) = duplex(1024);
        tokio::spawn(device(dev, reply));

        let (r, w) = split(host);
        Client::new(StreamLink::new(r, w)).with_timeout(Duration::from_millis(100))
    }

    /// A device that works.
    fn working(req: Req) -> Vec<Resp> {
        let body = match req.body {
            ReqBody::GetDeviceInfo => RespBody::DeviceInfo(DeviceInfo {
                chip_id: String::try_from("0123").unwrap(),
                firmware_version: String::try_from("v1").unwrap(),
            }),
            ReqBody::GetMeasurement => RespBody::Measurement(SAMPLE),
        };
        vec![Resp::new(req.id, body)]
    }

    #[tokio::test]
    async fn fetches_device_info() {
        let info = client(working).device_info().await.unwrap();

        assert_eq!(info.chip_id, "0123");
        assert_eq!(info.firmware_version, "v1");
    }

    #[tokio::test]
    async fn fetches_measurement() {
        let measurement = client(working).measurement().await.unwrap();

        assert_eq!(measurement.temperature, SAMPLE.temperature);
        assert_eq!(measurement.pressure, SAMPLE.pressure);
        assert_eq!(measurement.humidity, SAMPLE.humidity);
    }

    /// A stale reply ahead of the real one is skipped rather than returned.
    #[tokio::test]
    async fn skips_mismatched_responses() {
        let mut client = client(|req| {
            let mut resps = vec![Resp::new(
                req.id.wrapping_sub(1),
                RespBody::Error(DeviceError::MeasurementFailed),
            )];
            resps.extend(working(req));
            resps
        });

        assert!(client.measurement().await.is_ok());
    }

    #[tokio::test]
    async fn times_out_without_a_response() {
        let mut client = client(|_| vec![]);

        assert!(matches!(client.device_info().await, Err(Error::Timeout)));
    }

    #[tokio::test]
    async fn reports_device_errors() {
        let mut client = client(|req| {
            vec![Resp::new(
                req.id,
                RespBody::Error(DeviceError::MeasurementFailed),
            )]
        });

        assert!(matches!(
            client.measurement().await,
            Err(Error::Device(DeviceError::MeasurementFailed))
        ));
    }

    #[tokio::test]
    async fn rejects_the_wrong_kind_of_response() {
        let mut client = client(|req| vec![Resp::new(req.id, RespBody::Measurement(SAMPLE))]);

        assert!(matches!(
            client.device_info().await,
            Err(Error::UnexpectedResponse)
        ));
    }
}
