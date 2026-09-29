use std::process::ExitCode;

use clap::Parser;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{filter::Targets, fmt, prelude::*};

use red_lib::{Client, Error, Link, Transport};

/// A command line application for interacting with the RED device via UDP or USB.
#[derive(Debug, Clone, Parser)]
pub struct Args {
    /// The transport used to connect to the device
    #[clap(long, value_enum)]
    transport: Transport,

    /// A specific host to connect to / filter connections by.
    ///
    /// For USB, the serial number (chip id) of the device to open. For UDP, the chip id of a
    /// device to find with mDNS, or its address (`IP` or `IP:port`) to skip discovery.
    /// Optional when only one device is found.
    #[clap(long)]
    host: Option<String>,

    /// The command to execute
    #[clap(subcommand)]
    command: Command,

    /// The log level for the application
    #[clap(long, default_value_t = LevelFilter::INFO)]
    log_level: LevelFilter,
}

/// The set of commands that can be executed by the CLI application.
#[derive(Debug, Clone, Parser)]
pub enum Command {
    /// List attached devices
    ListDevices,
    /// Fetch device information
    GetDeviceInfo,
    /// Fetch a measurement from the device
    GetMeasurement,
}

#[tokio::main]
async fn main() -> ExitCode {
    // Parse command-line arguments
    let args = Args::parse();

    // Setup logging based on the provided log level, holding nusb and mdns_sd to warnings and
    // above as they narrate every open, close and probe at info
    let filter = Targets::new()
        .with_default(args.log_level)
        .with_target("nusb", args.log_level.min(LevelFilter::WARN))
        .with_target("mdns_sd", args.log_level.min(LevelFilter::WARN));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    match execute(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Connects over the selected transport and executes the command.
async fn execute(args: Args) -> Result<(), Error> {
    match args.transport {
        Transport::Usb => {
            if let Command::ListDevices = args.command {
                return list_usb().await;
            }

            let link = red_lib::usb::open(args.host.as_deref()).await?;
            run(Client::new(link), args.command).await
        }
        Transport::Udp => {
            if let Command::ListDevices = args.command {
                return list_udp().await;
            }

            let link = red_lib::udp::open(args.host.as_deref()).await?;
            run(Client::new(link), args.command).await
        }
    }
}

/// Prints the serial number of each attached USB device.
async fn list_usb() -> Result<(), Error> {
    let devices = red_lib::usb::list().await?;
    if devices.is_empty() {
        println!("no devices found");
    }
    for device in devices {
        println!("{}", device.serial_number().unwrap_or("<no serial number>"));
    }
    Ok(())
}

/// Prints the chip id, address and firmware version of each device found on the network.
async fn list_udp() -> Result<(), Error> {
    let devices = red_lib::udp::discover(red_lib::udp::DISCOVERY_TIMEOUT).await?;
    if devices.is_empty() {
        println!("no devices found");
    }
    for device in devices {
        println!(
            "{}  {}  {}",
            device.chip_id,
            device.addr,
            device.fw_version.as_deref().unwrap_or("<unknown version>")
        );
    }
    Ok(())
}

/// Executes a device command over an open connection.
async fn run<L: Link>(mut client: Client<L>, command: Command) -> Result<(), Error> {
    match command {
        Command::ListDevices => unreachable!("listing needs no connection"),
        Command::GetDeviceInfo => {
            let info = client.device_info().await?;
            println!("chip id:          {}", info.chip_id);
            println!("firmware version: {}", info.firmware_version);
        }
        Command::GetMeasurement => {
            let m = client.measurement().await?;
            println!("temperature: {:.2} °C", m.temperature);
            println!("pressure:    {:.2} hPa", m.pressure / 100.0);
            println!("humidity:    {:.2} %", m.humidity);
        }
    }
    Ok(())
}
