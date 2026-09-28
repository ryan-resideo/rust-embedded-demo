#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_stm32::gpio::Output;
use embassy_stm32::{bind_interrupts, dma, i2c, peripherals};
use embassy_time::{Duration, Ticker, Timer};

use defmt::{error, info, unwrap, warn};
use defmt_rtt as _; // global defmt logger (RTT transport)
use panic_probe as _; // panic handler (breakpoint; probe-rs reports the halt)

use red_core::Engine;
use red_fw::board::Board;
use red_fw::channels::{
    Request, RequestChannel, RequestReceiver, Response, ResponseChannel, Router, Source,
};
use red_fw::platform;

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
});

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
    let board = Board::init(Irqs);

    info!("red-fw starting, SYSCLK 180 MHz (8 MHz HSE bypass -> PLL)");

    // Spawn the LED task
    spawner.spawn(unwrap!(blink(board.led_g, board.led_b, board.led_r)));

    // TODO: other setups here

    // Create the platform and engine instances
    let platform = match platform::Stm32Platform::new(board.i2c).await {
        Ok(p) => p,
        Err(e) => {
            error!("failed to initialize platform: {:?}", e);
            #[allow(clippy::empty_loop)]
            loop {}
        }
    };
    let engine = Engine::new(platform);

    // Spawn the engine task
    spawner.spawn(unwrap!(run_engine(
        engine,
        ENGINE_REQUESTS.receiver(),
        engine_router()
    )));

    // TODO: spawn the transports and bind them to the engine channel
    info!("startup complete");
}

/// Engine task, services requests from every source and routes responses back to the source
#[embassy_executor::task]
async fn run_engine(
    mut engine: Engine<platform::Stm32Platform>,
    requests: RequestReceiver,
    responses: Router,
) {
    let mut ticker = Ticker::every(Duration::from_secs(10));

    loop {
        // Await incoming requests from the engine's request channel
        match select(ticker.next(), requests.receive()).await {
            Either::First(_) => {
                // Tick every 10 seconds
                if let Err(e) = engine.update().await {
                    error!("engine update failed: {:?}", e);
                }
            }
            Either::Second(req) => {
                let Request { address, req } = req;

                // Handle the request using the engine
                let resp = engine.handle_request(req).await;

                // Forward the response to the appropriate transport via the router
                if let Err(e) = responses.try_send(Response { address, resp }) {
                    warn!("dropped response to {}: {}", address, e);
                }
            }
        }
    }
}
