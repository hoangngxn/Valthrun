mod init;
pub use init::*;

mod modules;
pub use modules::*;

mod memory_read;
pub use memory_read::*;

mod memory_write;
pub use memory_write::*;

mod metrics;
pub use metrics::*;

mod input_keyboard;
pub use input_keyboard::*;

mod input_mouse;
pub use input_mouse::*;

mod process_protection;
pub use process_protection::*;

pub trait DriverCommand: Default + Copy {
    const COMMAND_ID: u32;
}

impl DriverCommand for DriverCommandInitialize {
    /* this command id should always be the same */
    const COMMAND_ID: u32 = 0x00;
}

impl DriverCommand for DriverCommandProcessModules {
    const COMMAND_ID: u32 = 0x01;
}

impl DriverCommand for DriverCommandProcessMemoryRead {
    const COMMAND_ID: u32 = 0x02;
}

impl DriverCommand for DriverCommandProcessMemoryWrite {
    const COMMAND_ID: u32 = 0x03;
}

impl DriverCommand for DriverCommandInputKeyboard {
    const COMMAND_ID: u32 = 0x04;
}

impl DriverCommand for DriverCommandInputMouse {
    const COMMAND_ID: u32 = 0x05;
}

impl DriverCommand for DriverCommandMetricsReportSend {
    const COMMAND_ID: u32 = 0x06;
}

impl DriverCommand for DriverCommandProcessProtection {
    const COMMAND_ID: u32 = 0x07;
}
