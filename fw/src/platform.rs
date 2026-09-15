use heapless::String;

use red_core::Platform;
use red_proto::resp::Measurement;

pub struct Stm32Platform {
    // TODO: BME280
}

impl Platform for Stm32Platform {
    fn chip_id(&self) -> String<32> {
        String::try_from(embassy_stm32::uid::uid_hex()).unwrap()
    }

    fn measure(&self) -> Measurement {
        todo!()
    }
}
