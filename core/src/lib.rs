#![no_std]

use heapless::String;

use red_proto::{
    req::{Req, ReqBody},
    resp::{DeviceInfo, Measurement, Resp, RespBody},
};

const GIT_VERSION: &str = env!("GIT_VERSION");

/// An abstract interface for the underlying hardware platform
pub trait Platform {
    /// Fetch the chip_id from the underlying platform
    fn chip_id(&self) -> String<32>;

    /// Perform a measurement using the underlying platform
    fn measure(&self) -> Measurement;
}

/// The engine that implements business / application logic
///
/// Defined here so it can be mocked and tested outside of the actual hardware platform.
pub struct Engine<P: Platform> {
    platform: P,
}

impl<P: Platform> Engine<P> {
    /// Creates a new engine with the given platform interface
    pub fn new(platform: P) -> Self {
        Self { platform }
    }

    /// The underlying hardware platform
    pub fn platform(&self) -> &P {
        &self.platform
    }

    /// Handle an incoming request, returning a response against the same identifier
    pub async fn handle_request(&self, req: Req) -> Resp {
        let body = match req.body {
            ReqBody::GetDeviceInfo => RespBody::DeviceInfo(DeviceInfo {
                chip_id: self.platform.chip_id(),
                firmware_version: String::try_from(GIT_VERSION).unwrap(),
            }),
            ReqBody::GetMeasurement => RespBody::Measurement(self.platform.measure()),
        };

        Resp::new(req.id, body)
    }
}

#[cfg(test)]
mod tests {
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};

    use super::*;

    struct MockPlatform;

    impl Platform for MockPlatform {
        fn chip_id(&self) -> String<32> {
            String::try_from("0123456789abcdef").unwrap()
        }

        fn measure(&self) -> Measurement {
            Measurement {
                temperature: 21.0,
                pressure: 1013.0,
                humidity: 55.0,
            }
        }
    }

    /// Resolves a future that never waits, as request handling does not yield.
    fn now<F: Future>(f: F) -> F::Output {
        match pin!(f).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("request handling should not block"),
        }
    }

    #[test]
    fn handles_device_info() {
        let engine = Engine::new(MockPlatform);

        let resp = now(engine.handle_request(Req::new(7, ReqBody::GetDeviceInfo)));

        // The response is tagged with the identifier of the request that produced it
        assert_eq!(resp.id, 7);

        let RespBody::DeviceInfo(info) = resp.body else {
            panic!("expected device info");
        };

        assert_eq!(info.chip_id, MockPlatform.chip_id());
        assert_eq!(info.firmware_version, GIT_VERSION);
    }

    #[test]
    fn handles_measurement() {
        let engine = Engine::new(MockPlatform);

        let resp = now(engine.handle_request(Req::new(9, ReqBody::GetMeasurement)));

        assert_eq!(resp.id, 9);

        let RespBody::Measurement(measurement) = resp.body else {
            panic!("expected measurement");
        };

        assert_eq!(measurement.temperature, MockPlatform.measure().temperature);
    }
}
