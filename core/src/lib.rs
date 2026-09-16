#![no_std]

use heapless::String;

use red_proto::{
    req::{Req, ReqBody},
    resp::{DeviceInfo, Error, Measurement, Resp, RespBody},
};

const GIT_VERSION: &str = env!("GIT_VERSION");

/// An abstract interface for the underlying hardware platform
///
/// `measure` is `async` because a real platform reads its sensor over a bus. No `Send`
/// bound is wanted here: the firmware runs on a single threaded embassy executor.
#[allow(async_fn_in_trait)]
pub trait Platform {
    /// Fetch the chip_id from the underlying platform
    fn chip_id(&self) -> String<32>;

    /// Perform a measurement using the underlying platform
    ///
    /// Returns `None` if the platform could not complete the measurement.
    async fn measure(&mut self) -> Option<Measurement>;
}

/// The engine that implements business / application logic
///
/// Defined here so it can be mocked and tested outside of the actual hardware platform.
pub struct Engine<P: Platform> {
    platform: P,
    last_measurement: Option<Measurement>,
}

impl<P: Platform> Engine<P> {
    /// Creates a new engine with the given platform interface
    pub fn new(platform: P) -> Self {
        Self {
            platform,
            last_measurement: None,
        }
    }

    /// The underlying hardware platform
    pub fn platform(&self) -> &P {
        &self.platform
    }

    /// Perform a periodic update, taking a sensor measurement and updating internal state.
    pub async fn update(&mut self) {
        match self.platform.measure().await {
            Some(measurement) => {
                self.last_measurement = Some(measurement);
            }
            None => {
                // Handle measurement failure if necessary
            }
        }
    }

    /// Handle an incoming request, returning a response against the same identifier
    pub async fn handle_request(&mut self, req: Req) -> Resp {
        let body = match req.body {
            ReqBody::GetDeviceInfo => RespBody::DeviceInfo(DeviceInfo {
                chip_id: self.platform.chip_id(),
                firmware_version: String::try_from(GIT_VERSION).unwrap(),
            }),
            ReqBody::GetMeasurement => match self.last_measurement.as_ref() {
                Some(measurement) => RespBody::Measurement(measurement.clone()),
                None => RespBody::Error(Error::MeasurementFailed),
            },
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

    struct MockPlatform {
        /// The measurement `measure` yields, or `None` to simulate a sensor that cannot be read.
        measurement: Option<Measurement>,
    }

    impl MockPlatform {
        const CHIP_ID: &'static str = "0123456789abcdef";
        const SAMPLE: Measurement = Measurement {
            temperature: 21.0,
            pressure: 1013.0,
            humidity: 55.0,
        };

        /// A platform whose sensor reads successfully.
        fn working() -> Self {
            Self {
                measurement: Some(Self::SAMPLE),
            }
        }

        /// A platform whose sensor cannot be read.
        fn failing() -> Self {
            Self { measurement: None }
        }
    }

    impl Platform for MockPlatform {
        fn chip_id(&self) -> String<32> {
            String::try_from(Self::CHIP_ID).unwrap()
        }

        async fn measure(&mut self) -> Option<Measurement> {
            self.measurement.clone()
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
        let mut engine = Engine::new(MockPlatform::working());

        let resp = now(engine.handle_request(Req::new(7, ReqBody::GetDeviceInfo)));

        // The response is tagged with the identifier of the request that produced it
        assert_eq!(resp.id, 7);

        let RespBody::DeviceInfo(info) = resp.body else {
            panic!("expected device info");
        };

        assert_eq!(info.chip_id, MockPlatform::CHIP_ID);
        assert_eq!(info.firmware_version, GIT_VERSION);
    }

    #[test]
    fn handles_measurement() {
        let mut engine = Engine::new(MockPlatform::working());

        let resp = now(engine.handle_request(Req::new(9, ReqBody::GetMeasurement)));

        assert_eq!(resp.id, 9);

        let RespBody::Measurement(measurement) = resp.body else {
            panic!("expected measurement");
        };

        assert_eq!(measurement.temperature, MockPlatform::SAMPLE.temperature);
        assert_eq!(measurement.pressure, MockPlatform::SAMPLE.pressure);
        assert_eq!(measurement.humidity, MockPlatform::SAMPLE.humidity);
    }

    /// A platform that cannot read its sensor answers the request rather than stalling it.
    #[test]
    fn reports_failed_measurement() {
        let mut engine = Engine::new(MockPlatform::failing());

        let resp = now(engine.handle_request(Req::new(11, ReqBody::GetMeasurement)));

        assert_eq!(resp.id, 11);
        assert!(matches!(
            resp.body,
            RespBody::Error(Error::MeasurementFailed)
        ));
    }
}
