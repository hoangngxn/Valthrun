use core::ptr;

use crate::types::{
    ProcessFilter,
    ProcessId,
    ProcessModuleInfo,
};

#[derive(Debug, Clone, Copy)]
pub struct DriverCommandProcessModules {
    // In:
    pub target_process: ProcessFilter,

    // In: Number of elements module_buffer can hold
    pub module_buffer_length: usize,

    // In:
    pub module_buffer: *mut ProcessModuleInfo,

    // Out:
    pub result: ProcessModulesResult,

    // Out: Total amount of modules the process has
    pub module_count: usize,

    // Out: The process id
    pub process_id: ProcessId,
}

#[derive(Debug, Clone, Copy)]
pub enum ProcessModulesResult {
    Success,
    BufferTooSmall,

    ProcessUnknown,
    ProcessUbiquitous,
}

impl Default for DriverCommandProcessModules {
    fn default() -> Self {
        Self {
            target_process: Default::default(),

            module_buffer_length: 0,
            module_buffer: ptr::null_mut(),

            result: ProcessModulesResult::ProcessUnknown,
            process_id: 0,
            module_count: 0,
        }
    }
}
