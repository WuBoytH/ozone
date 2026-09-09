use log::{info, log};

use crate::api::{hooks::*, memory::*};

#[no_mangle]
pub unsafe extern "C" fn A64HookFunction(symbol: *const u8, replace: *const u8, result: *mut *mut u8) {
    let ret = sky_Hook(symbol as _, replace as _, !result.is_null());

    if !result.is_null() {
        *result = ret as _;
    }
}

#[no_mangle]
pub unsafe extern "C" fn A64InlineHook(address: *const u8, callback: *const u8) {
    sky_InlineHook(address as _, callback as _)
}

#[no_mangle]
pub unsafe extern "C" fn getRegionAddress(region: Region) -> *const u8 {
    let info = sky_GetModuleInfo(ModuleIndex::Main as i32);

    match region {
        Region::Text => info.text.start,
        Region::Rodata => info.rodata.start,
        Region::Data => info.data.start,
        Region::Heap => s_Heap.start,
        _ => panic!("This region is not available"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skyline_tcp_send_raw(bytes: *const u8, size: usize) {
    let slice = std::slice::from_raw_parts(bytes, size);
    let string = std::str::from_utf8(slice).unwrap().to_owned();

    info!("{}", string)
}

#[no_mangle]
pub unsafe extern "C" fn sky_memcpy(dest: *mut u8, src: *const u8, size: usize) -> u32 {
    sky_Memcpy(dest, src, size)
}

/// Program ID of the running process. Used by skyline-rs `skyline::info::get_program_id`
/// (e.g. ARCropolis builds its chainloader NRR with it).
#[no_mangle]
pub extern "C" fn get_program_id() -> u64 {
    horizon_svc::get_program_id()
}

/// Returns the memory range of the plugin containing `internal_addr`, or null/null if none.
/// Used by skyline-rs `skyline::info::containing_plugin`.
#[no_mangle]
pub unsafe extern "C" fn get_plugin_addresses(internal_addr: *const u8, start: *mut *mut u8, end: *mut *mut u8) {
    let (plugin_start, plugin_end) = crate::loader::containing_plugin(internal_addr as usize).unwrap_or((0, 0));

    if !start.is_null() {
        *start = plugin_start as _;
    }
    if !end.is_null() {
        *end = plugin_end as _;
    }
}
