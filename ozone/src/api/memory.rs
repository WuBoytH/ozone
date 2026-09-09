#![allow(unused)]

extern "C" {
    pub fn sky_GetModuleInfo(index: i32) -> &'static ModuleInfo;
    pub fn sky_Memcpy(dest: *mut u8, src: *const u8, size: usize) -> u32;

    #[link_name = "_ZN3exl4util10mem_layout6s_HeapE"]
    pub static s_Heap: Range;

    /// Number of static (NSO) modules exlaunch discovered; valid indices for `sky_GetModuleInfo`.
    #[link_name = "_ZN3exl4util10mem_layout13s_ModuleCountE"]
    pub static s_ModuleCount: i32;
}

#[repr(u8)]
pub enum Region {
    Text,
    Rodata,
    Data,
    Bss,
    Heap,
}

#[repr(i32)]
pub enum ModuleIndex {
    Rtld,
    Main,
}

#[repr(C)]
pub struct Range {
    pub start: *const u8,
    pub size: usize,
}

#[repr(C)]
pub struct ModuleInfo {
    pub total: Range,
    pub text: Range,
    pub rodata: Range,
    pub data: Range,
}
