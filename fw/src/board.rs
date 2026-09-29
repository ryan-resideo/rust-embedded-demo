//! Board bring-up shared by the application and the on-device tests.
//!
//! Interrupts are deliberately *not* bound here. `bind_interrupts!` defines `#[no_mangle]`
//! handlers that the vector table resolves against weak defaults, and a strong definition
//! sitting in an rlib the linker never has a reason to pull would silently lose to that
//! default. Each binary binds its own and hands them to [`Board::init`].

use embassy_stm32::eth::{self, Ethernet, GenericPhy, PacketQueue, Sma};
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::i2c::{I2c, Master};
use embassy_stm32::interrupt::typelevel::Binding;
use embassy_stm32::mode::Async;
use embassy_stm32::rcc::{
    APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource, Sysclk,
};
use embassy_stm32::rng::{self, Rng};
use embassy_stm32::usb::{self, Driver};
use embassy_stm32::{Config, dma, i2c, interrupt, peripherals};
use static_cell::ConstStaticCell;

/// The interrupts [`Board::init`] needs bound to drive I2C1, its two DMA streams, USB OTG_FS,
/// Ethernet and the RNG.
///
/// Satisfied by any `bind_interrupts!` struct covering these seven; see `src/main.rs`.
pub trait Irqs:
    Binding<interrupt::typelevel::I2C1_EV, i2c::EventInterruptHandler<peripherals::I2C1>>
    + Binding<interrupt::typelevel::I2C1_ER, i2c::ErrorInterruptHandler<peripherals::I2C1>>
    + Binding<interrupt::typelevel::DMA1_STREAM6, dma::InterruptHandler<peripherals::DMA1_CH6>>
    + Binding<interrupt::typelevel::DMA1_STREAM0, dma::InterruptHandler<peripherals::DMA1_CH0>>
    + Binding<interrupt::typelevel::OTG_FS, usb::InterruptHandler<peripherals::USB_OTG_FS>>
    + Binding<interrupt::typelevel::ETH, eth::InterruptHandler>
    + Binding<interrupt::typelevel::HASH_RNG, rng::InterruptHandler<peripherals::RNG>>
    + Copy
{
}

impl<T> Irqs for T where
    T: Binding<interrupt::typelevel::I2C1_EV, i2c::EventInterruptHandler<peripherals::I2C1>>
        + Binding<interrupt::typelevel::I2C1_ER, i2c::ErrorInterruptHandler<peripherals::I2C1>>
        + Binding<interrupt::typelevel::DMA1_STREAM6, dma::InterruptHandler<peripherals::DMA1_CH6>>
        + Binding<interrupt::typelevel::DMA1_STREAM0, dma::InterruptHandler<peripherals::DMA1_CH0>>
        + Binding<interrupt::typelevel::OTG_FS, usb::InterruptHandler<peripherals::USB_OTG_FS>>
        + Binding<interrupt::typelevel::ETH, eth::InterruptHandler>
        + Binding<interrupt::typelevel::HASH_RNG, rng::InterruptHandler<peripherals::RNG>>
        + Copy
{
}

/// Scratch space the OTG driver stages received OUT packets in. It must hold the largest OUT
/// endpoint's max packet size; 256 bytes leaves room beyond the single 64 byte bulk endpoint.
static EP_OUT_BUFFER: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);

/// DMA descriptors and buffers for the Ethernet MAC, four full size frames each way (~12 KiB).
///
/// A static rather than a local: the DMA must be able to reach it, which rules out the
/// 64 KiB CCM RAM, and the linker only places statics there on request.
static PACKET_QUEUE: ConstStaticCell<PacketQueue<4, 4>> = ConstStaticCell::new(PacketQueue::new());

/// SMI address of the on-board LAN8742A, fixed by its PHYAD0 strap.
pub const PHY_ADDRESS: u8 = 0;

/// The Ethernet driver for this board: the MAC, with the LAN8742A behind SMI.
pub type Eth = Ethernet<'static, peripherals::ETH, GenericPhy<Sma<'static, peripherals::ETH_SMA>>>;

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
    /// Ethernet MAC and PHY on the RJ45 connector (CN14), ready to hand to `embassy_net`
    pub eth: Eth,
    /// Hardware RNG, to seed the network stack
    pub rng: Rng<'static, peripherals::RNG>,
}

impl Board {
    /// Initialises the clock tree and every peripheral the firmware uses.
    ///
    /// Panics if called twice: `embassy_stm32::init` takes the peripheral singleton, and the
    /// USB and Ethernet drivers take their static packet buffers.
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

        // Ethernet over RMII to the on-board LAN8742A. Nothing needs setting up beyond the
        // pins: the PHY supplies the 50 MHz reference clock, and the MAC's HCLK requirement
        // (at least 25 MHz for 100 Mbit) is met by the 168 MHz above.
        let sma = Sma::new(p.ETH_SMA, p.PA2, p.PC1); // MDIO, MDC
        let eth = Ethernet::new_with_phy(
            PACKET_QUEUE.take(),
            p.ETH,
            irqs,
            p.PA1,  // REF_CLK
            p.PA7,  // CRS_DV
            p.PC4,  // RXD0
            p.PC5,  // RXD1
            p.PG13, // TXD0
            p.PB13, // TXD1
            p.PG11, // TX_EN
            mac_address(),
            GenericPhy::new(sma, PHY_ADDRESS),
        );

        // The RNG runs from the PLL's 48 MHz Q output, shared with USB.
        let rng = Rng::new(p.RNG, irqs);

        Self {
            led_g,
            led_b,
            led_r,
            i2c,
            usb,
            eth,
            rng,
        }
    }
}

/// A MAC address unique to this board and stable across reboots, so DHCP hands the same
/// lease back.
///
/// The NUCLEO has no EUI-48 EEPROM, so the address is derived from the chip's 96 bit unique
/// id: its FNV-1a hash fills the low five bytes, and `0x02` marks it locally administered
/// and unicast.
pub fn mac_address() -> [u8; 6] {
    let hash = embassy_stm32::uid::uid()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        })
        .to_le_bytes();

    [0x02, hash[0], hash[1], hash[2], hash[3], hash[4]]
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
