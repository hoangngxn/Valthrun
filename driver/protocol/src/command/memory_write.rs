use core::ptr;

use crate::types::{
    MemoryAccessResult,
    ProcessId,
};

#[derive(Debug, Clone, Copy)]
pub struct DriverCommandProcessMemoryWrite {
    pub process_id: ProcessId,
    pub address: u64,

    pub buffer: *const u8,
    pub count: usize,

    pub result: MemoryAccessResult,
}

impl Default for DriverCommandProcessMemoryWrite {
    fn default() -> Self {
        Self {
            process_id: 0,
            address: 0,

            buffer: ptr::null_mut(),
            count: 0,

            result: Default::default(),
        }
    }
}
