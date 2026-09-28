#pragma once

#include <cstddef>

namespace exl::hook::nx64 {

    union GpRegister {
        u64 X;
        u32 W;
    };

    union GpRegisters {
        GpRegister m_Gp[31];
        struct {
            GpRegister _Gp[29];
            GpRegister m_Fp;
            GpRegister m_Lr;
        };
    };

    namespace impl {
        /* This type is only unioned with GpRegisters, so this is valid. */
        struct GpRegisterAccessorImpl {
            GpRegisters& Get() {
                return *reinterpret_cast<GpRegisters*>(this);
            }
        };

        struct GpRegisterAccessor64 : public GpRegisterAccessorImpl {
            u64& operator[](int index)
            {
                return Get().m_Gp[index].X;
            }
        };

        struct GpRegisterAccessor32 : public GpRegisterAccessorImpl {
            u32& operator[](int index)
            {
                return Get().m_Gp[index].W;
            }
        };
    }

    union VectorRegister {
        alignas(16) u8 m_Bytes[16];
        u64 m_D[2];
        u32 m_S[4];
        double m_Double[2];
        float m_Float[4];
    };

    /* Layout must match CTX_STACK_SIZE in inline_asm.s. The first 0x300 bytes are the
     * skyline-rs `InlineCtx` ABI (0x000 x0..x30, 0x0F8 sp, 0x100 q0..q31); the fields after
     * that are ozone-only and invisible to skyline plugins, which only see the 0x300-byte prefix.
     *   0x300 nzcv      condition flags at the hook site. Written back to NZCV after the
     *                   callback, so a callback may change them deliberately.
     *   0x308 lr backup scratch slot used by the entry's entrypoint (LrBackupOffset). */
    struct InlineCtx {
        union {
            /* Accessors are union'd with the gprs so that they can be accessed directly. */
            impl::GpRegisterAccessor64 X;
            impl::GpRegisterAccessor32 W;
            GpRegisters m_Gpr;
        };
        u64 m_Sp;
        VectorRegister m_Fpr[32];
        u64 m_Nzcv;
        u64 m_LrBackup;
    };
    static_assert(sizeof(InlineCtx) == 0x310, "InlineCtx must be 0x310 bytes (CTX_STACK_SIZE).");
    static_assert(offsetof(InlineCtx, m_Gpr.m_Lr) == 0xF0, "");
    static_assert(offsetof(InlineCtx, m_Sp) == 0xF8, "");
    static_assert(offsetof(InlineCtx, m_Fpr) == 0x100, "");
    static_assert(offsetof(InlineCtx, m_Nzcv) == 0x300, "");
    static_assert(offsetof(InlineCtx, m_LrBackup) == 0x308, "");

    void InitializeInline();
}