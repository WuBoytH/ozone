#include "lib.hpp"

extern "C" const exl::util::ModuleInfo& sky_GetModuleInfo(int index) {
    return exl::util::GetModuleInfo(index);
}

extern "C" uintptr_t sky_Hook(uintptr_t hook, uintptr_t callback, bool do_trampoline) {
    return exl::hook::arch::Hook(hook, callback, do_trampoline);
}

extern "C" void sky_InlineHook(uintptr_t hook, uintptr_t callback) {
    return exl::hook::nx64::HookInline(hook, callback);
}

extern "C" Result sky_Memcpy(uintptr_t dest, uintptr_t src, size_t n) {
    exl::util::RwPages page(dest, n);
    std::memcpy((void*)page.GetRw(), (void*)src, n);
    return 0;
}
