use clap::ValueEnum;

/// Transports supported by the RED comms library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Transport {
    /// Connect via USB
    Usb,
    /// Connect via IP/UDP
    Udp,
}

#[cfg(test)]
mod tests {}
