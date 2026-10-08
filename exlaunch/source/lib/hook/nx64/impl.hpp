#pragma once

#include "common.hpp"
#include "inline_impl.hpp"

namespace exl::hook::nx64 {

    void Initialize();

    /* allow_near: permit the 1-word `B` patch when in range (inline hooks only, see HookFuncImpl). */
    uintptr_t Hook(uintptr_t hook, uintptr_t callback, bool do_trampoline = false, bool allow_near = false);
    void HookInline(uintptr_t hook, uintptr_t callback);
}