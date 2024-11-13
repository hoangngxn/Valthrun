use bitflags::bitflags;

use crate::utils;

pub type ProcessId = u32;

bitflags! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct DriverFeature : u64 {
        const ProcessModules            = 0x00_00_00_01;
        const ProcessProtectionKernel   = 0x00_00_00_02;
        const ProcessProtectionZenith   = 0x00_00_00_04;

        const MemoryRead                = 0x00_00_01_00;
        const MemoryWrite               = 0x00_00_02_00;

        const InputKeyboard             = 0x00_01_00_00;
        const InputMouse                = 0x00_02_00_00;

        const Metrics                   = 0x01_00_00_00;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProcessFilter {
    None,
    Id { id: ProcessId },
    ImageBaseName { name: *const u8, name_length: usize },
}

impl Default for ProcessFilter {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessModuleInfo {
    pub base_dll_name: [u8; 0x100],
    pub base_address: u64,
    pub module_size: u64,
}

impl ProcessModuleInfo {
    pub fn get_base_dll_name(&self) -> Option<&str> {
        utils::fixed_buffer_to_str(&self.base_dll_name)
    }

    pub fn set_base_dll_name(&mut self, value: &str) -> bool {
        utils::str_to_fixed_buffer(&mut self.base_dll_name, value)
    }
}

impl Default for ProcessModuleInfo {
    fn default() -> Self {
        Self {
            base_dll_name: [0; 0x100],
            base_address: 0,
            module_size: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MemoryAccessResult {
    Success,
    PartialSuccess { bytes_copied: usize },

    ProcessUnknown,
}

impl Default for MemoryAccessResult {
    fn default() -> Self {
        MemoryAccessResult::ProcessUnknown
    }
}
