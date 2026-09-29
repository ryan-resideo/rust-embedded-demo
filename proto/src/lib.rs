//! Rust Embedded Demo - Protocol definitions
#![no_std]

pub mod req;

pub mod resp;

pub mod udp;

pub mod usb;

/// Largest encoded [`Req`](req::Req) or [`Resp`](resp::Resp), COBS delimiter included.
///
/// Sizes the encode buffers and frame accumulators on both ends of every transport.
pub const MAX_FRAME_LEN: usize = 128;

#[cfg(test)]
mod tests {
    use heapless::String;

    use super::*;
    use req::{Req, ReqBody};
    use resp::{DeviceInfo, Error, Measurement, Resp, RespBody};

    #[test]
    fn largest_response_fits_a_frame() {
        let longest = String::try_from("0123456789abcdef0123456789abcdef").unwrap();
        let resp = Resp::new(
            u32::MAX,
            RespBody::DeviceInfo(DeviceInfo {
                chip_id: longest.clone(),
                firmware_version: longest,
            }),
        );

        let mut buff = [0u8; MAX_FRAME_LEN];
        assert!(resp.encode(&mut buff).is_ok());
    }

    #[test]
    fn request_round_trip() {
        let mut buff = [0u8; MAX_FRAME_LEN];

        let n = Req::new(3, ReqBody::GetMeasurement)
            .encode(&mut buff)
            .unwrap();
        // COBS frames end with the delimiter, and it appears nowhere else
        assert_eq!(buff[n - 1], 0);
        assert!(!buff[..n - 1].contains(&0));

        let req = Req::decode(&mut buff[..n]).unwrap();
        assert_eq!(req.id, 3);
        assert!(matches!(req.body, ReqBody::GetMeasurement));
    }

    #[test]
    fn response_round_trip() {
        let mut buff = [0u8; MAX_FRAME_LEN];

        let sent = Resp::new(
            5,
            RespBody::Measurement(Measurement {
                temperature: 21.5,
                pressure: 101_325.0,
                humidity: 40.0,
            }),
        );
        let n = sent.encode(&mut buff).unwrap();

        let resp = Resp::decode(&mut buff[..n]).unwrap();
        assert_eq!(resp.id, 5);
        let RespBody::Measurement(m) = resp.body else {
            panic!("expected measurement");
        };
        assert_eq!(m.pressure, 101_325.0);

        let n = Resp::new(6, RespBody::Error(Error::MeasurementFailed))
            .encode(&mut buff)
            .unwrap();
        let resp = Resp::decode(&mut buff[..n]).unwrap();
        assert!(matches!(
            resp.body,
            RespBody::Error(Error::MeasurementFailed)
        ));
    }
}
