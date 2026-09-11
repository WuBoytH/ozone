#pragma once

#include "common.hpp"

#define EXL_MODULE_NAME "exlaunch"
#define EXL_MODULE_NAME_LEN 8

#define EXL_DEBUG
#define EXL_USE_FAKEHEAP

/*
#define EXL_SUPPORTS_REBOOTPAYLOAD
*/

namespace exl::setting {
    /* How large the fake .bss heap will be. */
    constexpr size_t HeapSize = 0x20000;

    /* How large the JIT area will be for hooks.
     * Each hook trampoline is 200 bytes and every inline hook also consumes one,
     * so 4 MiB gives ~20k hooks. Skyline plugins (ARCropolis chainloads, HDR, ...)
     * install thousands of hooks, and the original 0x10000 (~327 hooks) aborted with
     * HookTrampolineAllocFail. */
    constexpr size_t JitSize = 0x100000;

    /* How large the area will be inline hook pool. Each entry is 24 bytes. */
    constexpr size_t InlinePoolSize = 0x80000;

    /* Sanity checks. */
    static_assert(ALIGN_UP(JitSize, PAGE_SIZE) == JitSize, "");
    static_assert(ALIGN_UP(InlinePoolSize, PAGE_SIZE) == InlinePoolSize, "");
}
