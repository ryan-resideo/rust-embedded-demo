//! The NUCLEO-F429ZI implementation of [`Platform`].
//!
//! Environmental readings come from a Bosch BME280 on I2C1, wired to the Arduino header
//! (D15/PB8 = SCL, D14/PB9 = SDA).

use core::future::Future;

use bme280_rs::{AsyncBme280, Configuration, Oversampling, SensorMode};
use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::mode::Async;
use embassy_time::{Delay, Duration, with_timeout};
use heapless::String;

use red_core::Platform;
use red_proto::resp::Measurement;

/// I2C address of the BME280. (0x76, 0x77)
const BME280_ADDRESS: u8 = 0x76;

/// Timeout for sensor operations
const TIMEOUT: Duration = Duration::from_millis(100);

type Bme280 = AsyncBme280<I2c<'static, Async, Master>, Delay>;

pub struct Stm32Platform {
    bme280: Bme280,
}

impl Stm32Platform {
    /// Takes ownership of the I2C bus the sensor sits on.
    ///
    /// The sensor itself is configured lazily on the first measurement, so a board brought up
    /// without one attached still boots and still serves every other request.
    pub async fn new(i2c: I2c<'static, Async, Master>) -> Self {
        let mut bme280 = AsyncBme280::new_with_address(i2c, BME280_ADDRESS, Delay);

        // Check we can talk to the sensor by reading its chip ID.
        let _ = bme280.chip_id().await;

        // Initialise the sensor
        let _ = bme280.init().await;

        let configuration = Configuration::default()
            .with_temperature_oversampling(Oversampling::Oversample1)
            .with_pressure_oversampling(Oversampling::Oversample1)
            .with_humidity_oversampling(Oversampling::Oversample1)
            .with_sensor_mode(SensorMode::Normal);

        let _ = exchange(bme280.set_sampling_configuration(configuration)).await;

        Self { bme280 }
    }
}

impl Platform for Stm32Platform {
    fn chip_id(&self) -> String<32> {
        String::try_from(embassy_stm32::uid::uid_hex()).unwrap()
    }

    async fn measure(&mut self) -> Option<Measurement> {
        let sample = exchange(self.bme280.read_sample()).await.unwrap();

        Some(Measurement {
            temperature: sample.temperature?,
            pressure: sample.pressure?,
            humidity: sample.humidity?,
        })
    }
}

/// Runs one exchange with the sensor, collapsing both a timeout and an I2C error into `None`.
async fn exchange<T, E>(f: impl Future<Output = Result<T, E>>) -> Option<T> {
    with_timeout(TIMEOUT, f).await.ok()?.ok()
}
