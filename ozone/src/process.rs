mod handle {
    use super::{svcGetInfo, InfoType};
    
    pub fn get() -> u64 {
        let mut handle = 0;
        let result = unsafe { svcGetInfo(&mut handle, InfoType::MesosphereCurrentProcess, 0, 0) };
        handle
    }
}

extern "C" {
    fn svcGetInfo(out: &mut u64, info_id: InfoType, handle: u32, info_sub_id: u64) -> i32;
}

/// GetInfo IDs.
#[repr(u32)]
pub enum InfoType {
    /// Bitmask of allowed Core IDs.
    CoreMask = 0,                  
    PriorityMask = 1,              ///< Bitmask of allowed Thread Priorities.
    AliasRegionAddress = 2,        ///< Base of the Alias memory region.
    AliasRegionSize = 3,           ///< Size of the Alias memory region.
    HeapRegionAddress = 4,         ///< Base of the Heap memory region.
    HeapRegionSize = 5,            ///< Size of the Heap memory region.
    TotalMemorySize = 6,           ///< Total amount of memory available for process.
    UsedMemorySize = 7,            ///< Amount of memory currently used by process.
    DebuggerAttached = 8,          ///< Whether current process is being debugged.
    ResourceLimit = 9,             ///< Current process's resource limit handle.
    IdleTickCount = 10,            ///< Number of idle ticks on CPU.
    RandomEntropy = 11,            ///< [2.0.0+] Random entropy for current process.
    AslrRegionAddress = 12,        ///< [2.0.0+] Base of the process's address space.
    AslrRegionSize = 13,           ///< [2.0.0+] Size of the process's address space.
    StackRegionAddress = 14,       ///< [2.0.0+] Base of the Stack memory region.
    StackRegionSize = 15,          ///< [2.0.0+] Size of the Stack memory region.
    SystemResourceSizeTotal = 16,  ///< [3.0.0+] Total memory allocated for process memory management.
    SystemResourceSizeUsed = 17,   ///< [3.0.0+] Amount of memory currently used by process memory management.
    ProgramId = 18,                ///< [3.0.0+] Program ID for the process.
    InitialProcessIdRange = 19,    ///< [4.0.0-4.1.0] Min/max initial process IDs.
    UserExceptionContextAddress = 20,  ///< [5.0.0+] Address of the process's exception context (for break).
    TotalNonSystemMemorySize =
        21,  ///< [6.0.0+] Total amount of memory available for process, excluding that for process memory management.
    UsedNonSystemMemorySize =
        22,  ///< [6.0.0+] Amount of memory used by process, excluding that for process memory management.
    IsApplication = 23,  ///< [9.0.0+] Whether the specified process is an Application.
    MesosphereCurrentProcess       = 65001,
    ThreadTickCount = 0xF0000002,  //< Number of ticks spent on thread.
}