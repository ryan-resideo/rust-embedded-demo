use clap::Parser;
use tracing::level_filters::LevelFilter;

use red_lib::Transport;

/// A command line application for interacting with the RED device via UDP or USB.
#[derive(Debug, Clone, Parser)]
pub struct Args {
    /// The transport used to connect to the device
    #[clap(long, value_enum)]
    transport: Transport,

    /// A specific host to connect to / filter connections by
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
    /// Fetch device information
    GetDeviceInfo,
    /// Fetch a measurement from the device
    GetMeasurement,
}

#[tokio::main]
async fn main() {
    // Parse command-line arguments
    let args = Args::parse();

    // Setup logging based on the provided log level
    tracing_subscriber::fmt()
        .with_max_level(args.log_level)
        .init();

    println!("Hello, world!");

    // TODO: connect to the device

    // TODO: execute the command
}
