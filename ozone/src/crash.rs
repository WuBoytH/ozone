//! Crash diagnostics: a user exception handler (CPU faults, `brk` aborts) and a
//! logging hook for exlaunch aborts. Reports go to `sd:/ozone/crash_report.txt`,
//! the kernel debug log and the TCP logger, in that order of reliability.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use skyline::nn::os::{self, UserExceptionInfo};

use crate::api::memory::{sky_GetModuleInfo, s_ModuleCount};
use crate::logger::TcpLogger;

const HANDLER_STACK_SIZE: usize = 0x10000;
const REPORT_PATH: &str = "sd:/ultimate/ozone/crash_report.txt";

/// The frame nnSdk hands to the user exception handler.
///
/// nnsdk-rs declares `UserExceptionInfo` with an 8-byte pad after `PC` and 16-byte-aligned
/// NEON registers, but the real frame has a 16-byte pad and 8-byte-aligned NEON registers,
/// so everything from the NEON registers onwards is 8 bytes further along than the crate
/// thinks: read through the crate's struct, `ESR` was really `AFSR0` and `FAR` was
/// `AFSR1 | ESR << 32`. This mirrors the real layout (verified against the 2026-09-09
/// report, where the recovered ESR of 0x92000007 matched the read translation fault).
#[repr(C, align(16))]
struct ExceptionFrame {
    error_description: u32,
    _pad: [u32; 3],
    cpu_registers: [u64; 29],
    fp: u64,
    lr: u64,
    sp: u64,
    pc: u64,
    _padding: [u64; 2],
    fpu_registers: [[u64; 2]; 32],
    pstate: u32,
    afsr0: u32,
    afsr1: u32,
    esr: u32,
    far: u64,
}

const _: () = assert!(std::mem::size_of::<ExceptionFrame>() == 0x340);
// nnSdk writes into the buffer we register, so it must be at least as large as the crate's idea of it.
const _: () = assert!(std::mem::size_of::<ExceptionFrame>() >= std::mem::size_of::<UserExceptionInfo>());

#[repr(C, align(16))]
struct HandlerStack([u8; HANDLER_STACK_SIZE]);

static mut HANDLER_STACK: HandlerStack = HandlerStack([0; HANDLER_STACK_SIZE]);
static mut EXCEPTION_INFO: std::mem::MaybeUninit<ExceptionFrame> = std::mem::MaybeUninit::zeroed();
static SD_READY: AtomicBool = AtomicBool::new(false);

/// Installs the user exception handler. Safe to call at module init.
pub fn install() {
    unsafe {
        os::SetUserExceptionHandler(
            Some(exception_handler),
            std::ptr::addr_of_mut!(HANDLER_STACK).cast::<u8>(),
            HANDLER_STACK_SIZE as _,
            std::ptr::addr_of_mut!(EXCEPTION_INFO).cast::<UserExceptionInfo>(),
        );
    }
    println!("[ozone] User exception handler installed");
}

/// Called once the SD card is mounted: truncates the report file for this boot.
pub fn on_sd_mounted() {
    let _ = std::fs::create_dir_all("sd:/ultimate/ozone");
    match std::fs::File::create(REPORT_PATH) {
        Ok(mut file) => {
            let _ = writeln!(file, "ozone crash report for this boot");
            SD_READY.store(true, Ordering::SeqCst);
        },
        Err(e) => println!("[ozone] Could not create {}: {}", REPORT_PATH, e),
    }
}

/// Delivers one chunk of a report, synchronously on the calling thread, to every sink we
/// have. The SD file goes first because it survives the process being torn down; the TCP
/// socket is written directly rather than through the logger thread for the same reason.
/// Chunks are delivered as they are produced so a fault while building a later section
/// still leaves the earlier ones on record.
fn emit(chunk: &str) {
    if SD_READY.load(Ordering::SeqCst) {
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(REPORT_PATH) {
            let _ = file.write_all(chunk.as_bytes());
            let _ = file.flush();
        }
    }

    for line in chunk.lines() {
        let _ = horizon_svc::output_debug_string(line);
    }

    if !TcpLogger::write_direct(chunk) {
        // No client attached to the direct socket (or it is busy): queue it like any other log line.
        for line in chunk.lines() {
            log::info!("{}\n", line);
        }
    }
}

/// Reads the `.nx-module-name` entry at the start of an NSO's rodata, if it looks valid.
unsafe fn nso_module_name(rodata_start: *const u8) -> Option<String> {
    if rodata_start.is_null() {
        return None;
    }
    let len = *(rodata_start.add(4) as *const u32) as usize;
    if len == 0 || len > 0x80 {
        return None;
    }
    let bytes = std::slice::from_raw_parts(rodata_start.add(8), len);
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(len);
    let name = &bytes[..end];
    if name.is_empty() || !name.iter().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    Some(String::from_utf8_lossy(name).into_owned())
}

/// Describes `addr` as `module+offset` when it lies inside a plugin or a static module.
fn describe(addr: u64) -> Option<String> {
    if let Some((name, offset)) = crate::loader::plugin_for_address(addr as usize) {
        return Some(format!("{} + {:#x}", name, offset));
    }

    unsafe {
        let count = s_ModuleCount;
        for index in 0..count.max(0) {
            let info = sky_GetModuleInfo(index);
            let start = info.total.start as u64;
            let end = start + info.total.size as u64;
            if start <= addr && addr < end {
                let name = nso_module_name(info.rodata.start).unwrap_or_else(|| format!("module{}", index));
                return Some(format!("{} + {:#x}", name, addr - start));
            }
        }
    }

    None
}

fn annotate(addr: u64) -> String {
    match describe(addr) {
        Some(desc) => format!("{:#018x} ({})", addr, desc),
        None => format!("{:#018x}", addr),
    }
}

fn exception_type_name(desc: u32) -> &'static str {
    match desc {
        0x100 => "InstructionAbort",
        0x101 => "DataAbort",
        0x102 => "UnalignedInstruction",
        0x103 => "UnalignedData",
        0x104 => "UndefinedInstruction",
        0x105 => "ExceptionInstruction (brk / Rust abort or panic)",
        0x106 => "MemorySystemError",
        0x200 => "FpuException",
        0x301 => "InvalidSystemCall",
        0x302 => "SystemCallBreak",
        _ => "Unknown",
    }
}

fn esr_description(esr: u32) -> String {
    let ec = esr >> 26;
    let iss = esr & 0x1FF_FFFF;
    let fault_status = |fsc: u32| -> &'static str {
        match fsc >> 2 {
            0b0000 => "address size fault",
            0b0001 => "translation fault (unmapped)",
            0b0010 => "access flag fault",
            0b0011 => "permission fault (no rw/x permission)",
            _ => "other",
        }
    };
    match ec {
        0x00 => "unknown reason".to_string(),
        0x0e => "illegal execution state".to_string(),
        0x15 => "svc".to_string(),
        0x18 => "system register trap".to_string(),
        0x20 | 0x21 => format!("instruction abort, {}", fault_status(iss & 0x3F)),
        0x22 => "pc alignment fault".to_string(),
        0x24 | 0x25 => format!(
            "data abort on {}, {}",
            if iss & (1 << 6) != 0 { "write" } else { "read" },
            fault_status(iss & 0x3F)
        ),
        0x26 => "sp alignment fault".to_string(),
        0x2c => "floating point exception".to_string(),
        0x30 | 0x31 => "breakpoint".to_string(),
        0x32 | 0x33 => "software step".to_string(),
        0x34 | 0x35 => "watchpoint".to_string(),
        0x3c => format!("brk #{:#x}", iss & 0xFFFF),
        _ => format!("ec {:#x}", ec),
    }
}

/// Lists every 8-byte value above `sp` that points into known code. With frame
/// pointers omitted in Rust plugins this is the only backtrace we can get.
fn scan_stack(sp: u64, out: &mut String) {
    let Ok(region) = horizon_svc::query_memory(sp) else {
        let _ = writeln!(out, "  (stack region query failed)");
        return;
    };
    let region_end = region.addr + region.size;
    if region.perm & 1 == 0 || sp < region.addr || sp >= region_end {
        let _ = writeln!(out, "  (sp is not in readable memory)");
        return;
    }

    let start = sp & !7;
    let end = region_end.min(start + 0x1000);
    let mut found = 0;
    let mut addr = start;
    while addr < end {
        let value = unsafe { *(addr as *const u64) };
        if let Some(desc) = describe(value) {
            let _ = writeln!(out, "  [sp+{:#06x}] {:#018x} ({})", addr - sp, value, desc);
            found += 1;
        }
        addr += 8;
    }
    if found == 0 {
        let _ = writeln!(out, "  (no code pointers found in {:#x} bytes)", end - start);
    }
}

unsafe extern "C" fn exception_handler(info: *mut UserExceptionInfo) {
    // nnSdk filled our own `ExceptionFrame` buffer; see the type for why we don't read the crate's struct.
    let frame = &*info.cast::<ExceptionFrame>();

    let mut section = String::with_capacity(0x800);
    let _ = writeln!(section, "==================== ozone: user exception ====================");
    let _ = writeln!(
        section,
        "Type:  {:#x} ({})",
        frame.error_description,
        exception_type_name(frame.error_description)
    );
    let _ = writeln!(section, "ESR:   {:#010x} ({})", frame.esr, esr_description(frame.esr));
    let _ = writeln!(section, "FAR:   {}", annotate(frame.far));
    let _ = writeln!(section, "PC:    {}", annotate(frame.pc));
    let _ = writeln!(section, "LR:    {}", annotate(frame.lr));
    let _ = writeln!(section, "FP:    {:#018x}", frame.fp);
    let _ = writeln!(section, "SP:    {:#018x}", frame.sp);
    let _ = writeln!(section, "PSTATE {:#010x}", frame.pstate);
    emit(&section);

    section.clear();
    for (index, register) in frame.cpu_registers.iter().enumerate() {
        let _ = writeln!(section, "X[{:02}]: {}", index, annotate(*register));
    }
    emit(&section);

    section.clear();
    let _ = writeln!(section, "Code pointers on the stack (newest first):");
    scan_stack(frame.sp, &mut section);
    let _ = writeln!(section, "===============================================================");
    emit(&section);

    // Give the socket stack a moment to push the report out before nnSdk aborts.
    std::thread::sleep(Duration::from_secs(2));
}

/// Called by exlaunch (see `exlaunch/source/lib/diag/abort.cpp`) right before it aborts.
#[no_mangle]
pub unsafe extern "C" fn ozone_log_abort(
    file: *const std::ffi::c_char,
    line: i32,
    func: *const std::ffi::c_char,
    expr: *const std::ffi::c_char,
    value: u64,
) {
    let cstr = |ptr: *const std::ffi::c_char| -> String {
        if ptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };

    let mut report = String::new();
    let _ = writeln!(report, "==================== ozone: exlaunch abort ====================");
    let _ = writeln!(
        report,
        "value: {:#x} (module {}, description {})",
        value,
        value & 0x1FF,
        (value >> 9) & 0x1FFF
    );
    let _ = writeln!(report, "expr:  {}", cstr(expr));
    let _ = writeln!(report, "at:    {}:{} in {}", cstr(file), line, cstr(func));
    let _ = writeln!(report, "===============================================================");
    emit(&report);

    std::thread::sleep(Duration::from_secs(2));
}
