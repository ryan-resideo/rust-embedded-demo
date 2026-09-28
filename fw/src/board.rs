//! Board bring-up shared by the application and the on-device tests.
//!
//! Interrupts are deliberately *not* bound here. `bind_interrupts!` defines `#[no_mangle]`
//! handlers that the vector table resolves against weak defaults, and a strong definition
//! sitting in an rlib the linker never has a reason to pull would silently lose to that
//! default. Each binary binds its own and hands them to [`Board::init`].

use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::interrupt::typelevel::Binding;
use embassy_stm32::mode::Async;
use embassy_stm32::rcc::{
    APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllSource, Sysclk,
};
use embassy_stm32::{Config, dma, i2c, interrupt, peripherals};

/// The interrupts [`Board::init`] needs bound to drive I2C1 and its two DMA streams.
///
/// Satisfied by any `bind_interrupts!` struct covering these four; see `src/main.rs`.
pub trait Irqs:
    Binding<interrupt::typelevel::I2C1_EV, i2c::EventInterruptHandler<peripherals::I2C1>>
    + Binding<interrupt::typelevel::I2C1_ER, i2c::ErrorInterruptHandler<peripherals::I2C1>>
    + Binding<interrupt::typelevel::DMA1_STREAM6, dma::InterruptHandler<peripherals::DMA1_CH6>>
    + Binding<interrupt::typelevel::DMA1_STREAM0, dma::InterruptHandler<peripherals::DMA1_CH0>>
{
}

impl<T> Irqs for T where
    T: Binding<interrupt::typelevel::I2C1_EV, i2c::EventInterruptHandler<peripherals::I2C1>>
        + Binding<interrupt::typelevel::I2C1_ER, i2c::ErrorInterruptHandler<peripherals::I2C1>>
        + Binding<interrupt::typelevel::DMA1_STREAM6, dma::InterruptHandler<peripherals::DMA1_CH6>>
        + Binding<interrupt::typelevel::DMA1_STREAM0, dma::InterruptHandler<peripherals::DMA1_CH0>>
{
}

/// The peripherals the firmware uses, brought up and ready.
pub struct Board {
    /// LD1 (green)
    pub led_g: Output<'static>,
    /// LD2 (blue)
    pub led_b: Output<'static>,
    /// LD3 (red)
    pub led_r: Output<'static>,
    /// I2C1 on the Arduino header, the bus the BME280 sits on
    pub i2c: I2c<'static, Async, Master>,
}

impl Board {
    /// Initialises the clock tree and every peripheral the firmware uses.
    ///
    /// Panics if called twice: `embassy_stm32::init` takes the peripheral singleton.
    pub fn init(irqs: impl Irqs + 'static) -> Self {
        let p = embassy_stm32::init(clock_config());

        let led_g = Output::new(p.PB0, Level::Low, Speed::Low);
        let led_b = Output::new(p.PB7, Level::Low, Speed::Low);
        let led_r = Output::new(p.PB14, Level::Low, Speed::Low);

        // I2C1 on the Arduino header: D15/PB8 = SCL, D14/PB9 = SDA, both AF4.
        // The default config is 100 kHz with the internal pullups off, so the sensor breakout
        // is expected to provide them.
        let i2c = I2c::new(
            p.I2C1,
            p.PB8,
            p.PB9,
            p.DMA1_CH6,
            p.DMA1_CH0,
            irqs,
            Default::default(),
        );

        Self {
            led_g,
            led_b,
            led_r,
            i2c,
        }
    }
}

/// 8 MHz HSE (ST-Link MCO, bypass) -> PLL -> 180 MHz SYSCLK.
pub fn clock_config() -> Config {
    let mut config = Config::default();
    config.rcc.hse = Some(Hse {
        freq: embassy_stm32::time::Hertz(8_000_000),
        mode: HseMode::Bypass,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV4,
        mul: PllMul::MUL180,
        divp: Some(PllPDiv::DIV2), // 8 / 4 * 180 / 2 = 180 MHz
        divq: None,
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.apb1_pre = APBPrescaler::DIV4; // 45 MHz, within the 45 MHz max
    config.rcc.apb2_pre = APBPrescaler::DIV2; // 90 MHz, within the 90 MHz max
    config
}
