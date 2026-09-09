/*
 * Copyright (c) Atmosphère-NX
 *
 * This program is free software; you can redistribute it and/or modify it
 * under the terms and conditions of the GNU General Public License,
 * version 2, as published by the Free Software Foundation.
 *
 * This program is distributed in the hope it will be useful, but WITHOUT
 * ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 * FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License for
 * more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

#include "abort.hpp"

#include <cstring>
#include <algorithm>
#include <atomic>

/* Implemented in ozone (Rust). Logs the abort over TCP / to the SD before we die. */
extern "C" void ozone_log_abort(const char *file, int line, const char *func, const char *expr, u64 value);

namespace exl::diag {

    static void LogAbort(const char *file, int line, const char *func, const char *expr, u64 value) {
        /* Never recurse if the logger itself aborts. */
        static std::atomic<bool> s_Logging;
        if (s_Logging.exchange(true))
            return;
        ozone_log_abort(file, line, func, expr, value);
    }

    void NORETURN NOINLINE AbortImpl(const AbortCtx & ctx) {
        #ifdef EXL_SUPPORTS_REBOOTPAYLOAD
        /* Ensure abort handler doesn't recursively abort. */
        static std::atomic<bool> recurse_guard;
        auto recursing = recurse_guard.exchange(true);

        if(!recursing && util::IsSocErista()) {
            /* Reboot to abort payload.*/
            AbortToPayload(ctx);
        } else 
        #endif
        {
            /* We have no capability of chainloading payloads on mariko. */
            /* Don't have a great solution for this at the moment, just data abort. */
            /* TODO: maybe write to a file? custom fatal program? */
            register u64 addr __asm__("x27") = 0x6969696969696969;
            register u64 val __asm__("x28")  = ctx.m_Result;
            while (true) {
                __asm__ __volatile__ (
                    "str %[val], [%[addr]]"
                    :
                    : [val]"r"(val), [addr]"r"(addr)
                );
            }
        }

        UNREACHABLE;
    }

    #define ABORT_WITH_VALUE(v)                             \
    {                                                       \
        LogAbort(file, line, func, expr, (u64)(v));         \
        exl::diag::AbortCtx ctx {.m_Result = (Result)v};    \
        AbortImpl(ctx);                                     \
    }

    /* TODO: better assert/abort support. */
    void NORETURN NOINLINE AssertionFailureImpl(const char *file, int line, const char *func, const char *expr, u64 value, const char *format, ...) ABORT_WITH_VALUE(value)
    void NORETURN NOINLINE AssertionFailureImpl(const char *file, int line, const char *func, const char *expr, u64 value)                          ABORT_WITH_VALUE(value)
    void NORETURN NOINLINE AbortImpl(const char *file, int line, const char *func, const char *expr, u64 value, const char *format, ...)            ABORT_WITH_VALUE(value)
    void NORETURN NOINLINE AbortImpl(const char *file, int line, const char *func, const char *expr, u64 value)                                     ABORT_WITH_VALUE(value)

};

/* C shim for libnx */
extern "C" NORETURN void exl_abort(Result r) {
    exl::diag::LogAbort(NULL, 0, NULL, "exl_abort", (u64)r);
    exl::diag::AbortCtx ctx {.m_Result = r};
    exl::diag::AbortImpl(ctx);
}