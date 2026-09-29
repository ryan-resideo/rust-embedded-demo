//! On-device tests, run against an attached probe with `cargo test`.
//!
//! These exercise real hardware: a NUCLEO-F429ZI with a BME280 on I2C1 must be connected,
//! so they fail on a bare board by design. defmt-test provides the entry point and exits via
//! semihosting, which probe-rs turns back into a process exit code.

#![no_std]
#![no_main]

use defmt_rtt as _; // global defmt logger (RTT transport)
use panic_probe as _; // panic handler; defmt-test reports failures through it

use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::mode::Async;
use embassy_stm32::{bind_interrupts, dma, i2c, peripherals, usb};

use red_fw::board::Board;

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

/// Hardware brought up once, shared by every test in the suite.
struct State {
    /// Taken by the first test that needs to own the bus.
    i2c: Option<I2c<'static, Async, Master>>,
}

#[defmt_test::tests]
mod tests {
    use bme280_rs::{AsyncBme280, CHIP_ID};
    use embassy_futures::block_on;
    use embassy_time::Delay;

    use red_core::Platform;
    use red_fw::platform::{BME280_ADDRESS, Stm32Platform};

    use super::{Board, Irqs, State};

    #[init]
    fn init() -> State {
        let board = Board::init(Irqs);

        State {
            i2c: Some(board.i2c),
        }
    }

    /// Reads the BME280's chip ID register over I2C1.
    ///
    /// The cheapest end-to-end check there is: clock tree, pin mux, DMA, interrupts and the
    /// sensor itself all have to work for the right byte to come back.
    ///
    /// The driver wants the bus by value, so it borrows it for the length of the test via
    /// embedded-hal-async's `&mut T` impl and leaves the state intact for the tests below.
    #[test]
    fn bme280_chip_id(state: &mut State) {
        let i2c = state.i2c.as_mut().unwrap();
        let mut bme280 = AsyncBme280::new_with_address(i2c, BME280_ADDRESS, Delay);

        let chip_id = block_on(bme280.chip_id()).unwrap();

        defmt::assert_eq!(chip_id, CHIP_ID);
    }

    /// The 96 bit unique ID behind `Platform::chip_id` is populated.
    ///
    /// Reads the die's factory programmed UID, so it needs no peripheral beyond the core.
    #[test]
    fn stm32_uid() {
        let uid = embassy_stm32::uid::uid_hex();

        defmt::assert_eq!(uid.len(), 24);
        defmt::assert!(uid.bytes().any(|b| b != b'0'));
    }

    /// The platform initialises and returns a plausible measurement.
    ///
    /// Takes the bus, so it must run after every test that borrows it.
    #[test]
    fn platform_measures(state: &mut State) {
        let i2c = state.i2c.take().unwrap();

        let mut platform = block_on(Stm32Platform::new(i2c)).unwrap();

        // The sensor needs a conversion cycle in normal mode before the first sample.
        block_on(embassy_time::Timer::after_millis(50));

        let measurement = block_on(platform.measure()).unwrap();
        defmt::info!("measurement: {:?}", measurement);

        defmt::assert!(measurement.temperature > -40.0 && measurement.temperature < 85.0);
        // Pressure is in pascals: ~30 kPa (Everest) to ~110 kPa covers anywhere on Earth.
        defmt::assert!(measurement.pressure > 30_000.0 && measurement.pressure < 110_000.0);
        defmt::assert!(measurement.humidity >= 0.0 && measurement.humidity <= 100.0);
    }
}
