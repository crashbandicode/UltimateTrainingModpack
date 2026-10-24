// Exact site main+6510f4. x16/x17/NZCV are dead at the detour entry.
// Preserve every other GPR, all vector bits, FPCR/FPSR, and original native SP.
.section .text.mii_ordinary_gate,"ax",%progbits
.p2align 2
.global mii_ordinary_gate_entry
.hidden mii_ordinary_gate_entry
.type mii_ordinary_gate_entry,%function
mii_ordinary_gate_entry:
    sub sp, sp, #0x320
    stp x0, x1, [sp, #0x0]
    stp x2, x3, [sp, #0x10]
    stp x4, x5, [sp, #0x20]
    stp x6, x7, [sp, #0x30]
    stp x8, x9, [sp, #0x40]
    stp x10, x11, [sp, #0x50]
    stp x12, x13, [sp, #0x60]
    stp x14, x15, [sp, #0x70]
    stp x16, x17, [sp, #0x80]
    stp x18, x19, [sp, #0x90]
    stp x20, x21, [sp, #0xa0]
    stp x22, x23, [sp, #0xb0]
    stp x24, x25, [sp, #0xc0]
    stp x26, x27, [sp, #0xd0]
    stp x28, x29, [sp, #0xe0]
    str x30, [sp, #0xf0]
    stp q0, q1, [sp, #0x100]
    stp q2, q3, [sp, #0x120]
    stp q4, q5, [sp, #0x140]
    stp q6, q7, [sp, #0x160]
    stp q8, q9, [sp, #0x180]
    stp q10, q11, [sp, #0x1a0]
    stp q12, q13, [sp, #0x1c0]
    stp q14, q15, [sp, #0x1e0]
    stp q16, q17, [sp, #0x200]
    stp q18, q19, [sp, #0x220]
    stp q20, q21, [sp, #0x240]
    stp q22, q23, [sp, #0x260]
    stp q24, q25, [sp, #0x280]
    stp q26, q27, [sp, #0x2a0]
    stp q28, q29, [sp, #0x2c0]
    stp q30, q31, [sp, #0x2e0]
    mrs x9, nzcv
    str x9, [sp, #0x300]
    mrs x9, fpcr
    str x9, [sp, #0x308]
    mrs x9, fpsr
    str x9, [sp, #0x310]
    mov x0, x19
    mov x1, x21
    add x2, sp, #0x320
    bl mii_ordinary_gate_decide
    str x0, [sp, #0x318]
    ldr x9, [sp, #0x308]
    msr fpcr, x9
    ldr x9, [sp, #0x310]
    msr fpsr, x9
    ldr x9, [sp, #0x300]
    msr nzcv, x9
    ldp q0, q1, [sp, #0x100]
    ldp q2, q3, [sp, #0x120]
    ldp q4, q5, [sp, #0x140]
    ldp q6, q7, [sp, #0x160]
    ldp q8, q9, [sp, #0x180]
    ldp q10, q11, [sp, #0x1a0]
    ldp q12, q13, [sp, #0x1c0]
    ldp q14, q15, [sp, #0x1e0]
    ldp q16, q17, [sp, #0x200]
    ldp q18, q19, [sp, #0x220]
    ldp q20, q21, [sp, #0x240]
    ldp q22, q23, [sp, #0x260]
    ldp q24, q25, [sp, #0x280]
    ldp q26, q27, [sp, #0x2a0]
    ldp q28, q29, [sp, #0x2c0]
    ldp q30, q31, [sp, #0x2e0]
    ldp x0, x1, [sp, #0x0]
    ldp x2, x3, [sp, #0x10]
    ldp x4, x5, [sp, #0x20]
    ldp x6, x7, [sp, #0x30]
    ldp x8, x9, [sp, #0x40]
    ldp x10, x11, [sp, #0x50]
    ldp x12, x13, [sp, #0x60]
    ldp x14, x15, [sp, #0x70]
    ldp x16, x17, [sp, #0x80]
    ldp x18, x19, [sp, #0x90]
    ldp x20, x21, [sp, #0xa0]
    ldp x22, x23, [sp, #0xb0]
    ldp x24, x25, [sp, #0xc0]
    ldp x26, x27, [sp, #0xd0]
    ldp x28, x29, [sp, #0xe0]
    ldr x30, [sp, #0xf0]
    ldr x17, [sp, #0x318]
    add sp, sp, #0x320
    br x17
.size mii_ordinary_gate_entry,.-mii_ordinary_gate_entry
