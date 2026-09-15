#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::rcc::{
    APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllSource, Sysclk,
};
use embassy_time::{Duration, Timer};

use panic_probe as _; // panic handler (breakpoint; probe-rs reports the halt)

use red_core::Engine;

mod channels;
mod platform;

use channels::{
    Request, RequestChannel, RequestReceiver, Response, ResponseChannel, Router, Source,
};

/// Requests into the engine, shared by every communications source.
static ENGINE_REQUESTS: RequestChannel = RequestChannel::new();

/// Responses out to the USB source.
static USB_RESPONSES: ResponseChannel = ResponseChannel::new();

/// Responses out to the UDP source.
static UDP_RESPONSES: ResponseChannel = ResponseChannel::new();

/// The response routes the engine uses to reach each supported transport.
fn engine_router() -> Router {
    let mut router = Router::new();

    router
        .register(Source::Usb, USB_RESPONSES.sender())
        .register(Source::Udp, UDP_RESPONSES.sender());

    router
}

/// LD1 (green) on the NUCLEO-F429ZI.
#[embassy_executor::task]
async fn blink(mut led_g: Output<'static>, mut led_b: Output<'static>, mut led_r: Output<'static>) {
    loop {
        led_g.set_high();
        Timer::after(Duration::from_millis(500)).await;
        led_g.set_low();
        Timer::after(Duration::from_millis(500)).await;
        led_b.set_high();
        Timer::after(Duration::from_millis(500)).await;
        led_b.set_low();
        Timer::after(Duration::from_millis(500)).await;

        led_r.set_high();
        Timer::after(Duration::from_millis(500)).await;
        led_r.set_low();
        Timer::after(Duration::from_millis(500)).await;
    }
}

/// Main task, setup peripherals and spawn tasks.
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(clock_config());

    // Setup and spawn the LED task
    let led_g = Output::new(p.PB0, Level::Low, Speed::Low);
    let led_b = Output::new(p.PB7, Level::Low, Speed::Low);
    let led_r = Output::new(p.PB14, Level::Low, Speed::Low);
    spawner.spawn(blink(led_g, led_b, led_r).unwrap());

    // TODO: other setups here

    // Create the platform and engine instances
    let platform = platform::Stm32Platform {};
    let engine = Engine::new(platform);
    spawner.spawn(run_engine(engine, ENGINE_REQUESTS.receiver(), engine_router()).unwrap());

    // TODO: spawn the transports and bind them to the engine channel
}

/// Engine task, services requests from every source and routes responses back to the source
#[embassy_executor::task]
async fn run_engine(
    engine: Engine<platform::Stm32Platform>,
    requests: RequestReceiver,
    responses: Router,
) {
    loop {
        // Await incoming requests from the engine's request channel
        let Request { address, req } = requests.receive().await;

        // Handle the request using the engine
        let resp = engine.handle_request(req).await;

        // Forward the response to the appropriate transport via the router
        let _ = responses.try_send(Response { address, resp });
    }
}

/// 8 MHz HSE (ST-Link MCO, bypass) -> PLL -> 180 MHz SYSCLK.
fn clock_config() -> Config {
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
