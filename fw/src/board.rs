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
    APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource, Sysclk,
};
use embassy_stm32::usb::{self, Driver};
use embassy_stm32::{Config, dma, i2c, interrupt, peripherals};
use static_cell::ConstStaticCell;

/// The interrupts [`Board::init`] needs bound to drive I2C1, its two DMA streams and USB OTG_FS.
///
/// Satisfied by any `bind_interrupts!` struct covering these five; see `src/main.rs`.
pub trait Irqs:
    Binding<interrupt::typelevel::I2C1_EV, i2c::EventInterruptHandler<peripherals::I2C1>>
    + Binding<interrupt::typelevel::I2C1_ER, i2c::ErrorInterruptHandler<peripherals::I2C1>>
    + Binding<interrupt::typelevel::DMA1_STREAM6, dma::InterruptHandler<peripherals::DMA1_CH6>>
    + Binding<interrupt::typelevel::DMA1_STREAM0, dma::InterruptHandler<peripherals::DMA1_CH0>>
    + Binding<interrupt::typelevel::OTG_FS, usb::InterruptHandler<peripherals::USB_OTG_FS>>
    + Copy
{
}

impl<T> Irqs for T where
    T: Binding<interrupt::typelevel::I2C1_EV, i2c::EventInterruptHandler<peripherals::I2C1>>
        + Binding<interrupt::typelevel::I2C1_ER, i2c::ErrorInterruptHandler<peripherals::I2C1>>
        + Binding<interrupt::typelevel::DMA1_STREAM6, dma::InterruptHandler<peripherals::DMA1_CH6>>
        + Binding<interrupt::typelevel::DMA1_STREAM0, dma::InterruptHandler<peripherals::DMA1_CH0>>
        + Binding<interrupt::typelevel::OTG_FS, usb::InterruptHandler<peripherals::USB_OTG_FS>>
        + Copy
{
}

/// Scratch space the OTG driver stages received OUT packets in. It must hold the largest OUT
/// endpoint's max packet size; 256 bytes leaves room beyond the single 64 byte bulk endpoint.
static EP_OUT_BUFFER: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);

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
    /// USB OTG_FS on the user USB connector (CN13), ready to hand to `embassy_usb`
    pub usb: Driver<'static, peripherals::USB_OTG_FS>,
}

impl Board {
    /// Initialises the clock tree and every peripheral the firmware uses.
    ///
    /// Panics if called twice: `embassy_stm32::init` takes the peripheral singleton, and the
    /// USB driver takes its static packet buffer.
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

        // USB OTG_FS on CN13: PA11 = D-, PA12 = D+. The board is powered from the ST-Link
        // whether or not CN13 is plugged in, so the USB spec requires sensing VBUS (PA9) to
        // notice the cable coming and going.
        let mut usb_config = usb::Config::default();
        usb_config.vbus_detection = true;
        let usb = Driver::new_fs(
            p.USB_OTG_FS,
            irqs,
            p.PA12,
            p.PA11,
            EP_OUT_BUFFER.take(),
            usb_config,
        );

        Self {
            led_g,
            led_b,
            led_r,
            i2c,
            usb,
        }
    }
}

/// 8 MHz HSE (ST-Link MCO, bypass) -> PLL -> 168 MHz SYSCLK, 48 MHz USB.
///
/// OTG_FS needs exactly 48 MHz, and on the F429 the only source is the main PLL's Q output.
/// That rules out the 180 MHz maximum: its 360 MHz VCO has no integer divisor to 48 MHz,
/// whereas a 336 MHz VCO gives 168 MHz on P and 48 MHz on Q.
pub fn clock_config() -> Config {
    let mut config = Config::default();
    config.rcc.hse = Some(Hse {
        freq: embassy_stm32::time::Hertz(8_000_000),
        mode: HseMode::Bypass,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV4,
        mul: PllMul::MUL168,
        divp: Some(PllPDiv::DIV2), // 8 / 4 * 168 / 2 = 168 MHz
        divq: Some(PllQDiv::DIV7), // 8 / 4 * 168 / 7 = 48 MHz
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.apb1_pre = APBPrescaler::DIV4; // 42 MHz, within the 45 MHz max
    config.rcc.apb2_pre = APBPrescaler::DIV2; // 84 MHz, within the 90 MHz max
    config
}
