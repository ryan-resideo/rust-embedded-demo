//! The NUCLEO-F429ZI implementation of [`Platform`].
//!
//! Environmental readings come from a Bosch BME280 on I2C1, wired to the Arduino header
//! (D15/PB8 = SCL, D14/PB9 = SDA).

use bme280_rs::{AsyncBme280, Configuration, Oversampling, SensorMode};
use defmt::{debug, error};
use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::mode::Async;
use embassy_time::{Delay, Duration, with_timeout};
use heapless::String;

use red_core::{Platform, PlatformError};
use red_proto::resp::Measurement;

/// I2C address of the BME280. (0x76, 0x77)
const BME280_ADDRESS: u8 = 0x77; // 👀

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
    pub async fn new(i2c: I2c<'static, Async, Master>) -> Result<Self, PlatformError> {
        let mut bme280 = AsyncBme280::new_with_address(i2c, BME280_ADDRESS, Delay);

        // Check we can talk to the sensor by reading its chip ID.
        match with_timeout(TIMEOUT, bme280.chip_id()).await {
            Ok(Ok(chip_id)) => {
                debug!("bme280: chip id: {}", chip_id);
            }
            Ok(Err(e)) => {
                error!("bme280: failed to read chip ID: {:?}", e);
                return Err(PlatformError::I2cError);
            }
            Err(e) => {
                error!("bme280: timeout reading chip ID: {:?}", e);
                return Err(PlatformError::Timeout);
            }
        }

        // Initialise the sensor
        match with_timeout(Duration::from_secs(2), bme280.init()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                error!("bme280: failed to initialize: {:?}", e);
                return Err(PlatformError::I2cError);
            }
            Err(e) => {
                error!("bme280: timeout initializing: {:?}", e);
                return Err(PlatformError::Timeout);
            }
        }

        let configuration = Configuration::default()
            .with_temperature_oversampling(Oversampling::Oversample1)
            .with_pressure_oversampling(Oversampling::Oversample1)
            .with_humidity_oversampling(Oversampling::Oversample1)
            .with_sensor_mode(SensorMode::Normal);

        match with_timeout(TIMEOUT, bme280.set_sampling_configuration(configuration)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                error!("bme280: failed to set sampling configuration: {:?}", e);
                return Err(PlatformError::I2cError);
            }
            Err(e) => {
                error!("bme280: timeout setting sampling configuration: {:?}", e);
                return Err(PlatformError::Timeout);
            }
        }

        Ok(Self { bme280 })
    }
}

impl Platform for Stm32Platform {
    fn chip_id(&self) -> String<32> {
        String::try_from(embassy_stm32::uid::uid_hex()).unwrap()
    }

    async fn measure(&mut self) -> Result<Measurement, PlatformError> {
        let sample = match with_timeout(TIMEOUT, self.bme280.read_sample()).await {
            Ok(Ok(sample)) => sample,
            Ok(Err(e)) => {
                error!("bme280: failed to read sample: {:?}", e);
                return Err(PlatformError::I2cError);
            }
            Err(e) => {
                error!("bme280: timeout reading sample: {:?}", e);
                return Err(PlatformError::Timeout);
            }
        };

        Ok(Measurement {
            temperature: sample.temperature.ok_or(PlatformError::I2cError)?,
            pressure: sample.pressure.ok_or(PlatformError::I2cError)?,
            humidity: sample.humidity.ok_or(PlatformError::I2cError)?,
        })
    }
}
