use core::ptr;

use crate::types::{
    MemoryAccessResult,
    ProcessId,
};

#[derive(Debug, Clone, Copy)]
pub struct DriverCommandProcessMemoryRead {
    pub process_id: ProcessId,
    pub address: u64,

    pub buffer: *mut u8,
    pub count: usize,

    pub result: MemoryAccessResult,
}

impl Default for DriverCommandProcessMemoryRead {
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
