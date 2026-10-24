// Function entry main+650010, before its native prologue or any Rust call.
// Dispatcher1755e7c -> functor651640 tail BR preserves x19/x20/x21/LR.
// Native input is FE in x0; return x0 is forwarded without modification.
.section .text.native_service,"ax",%progbits
.p2align 2
.global training_mod_native_service_entry
.hidden training_mod_native_service_entry
.hidden training_mod_native_service_dispatch
.type training_mod_native_service_entry,%function
training_mod_native_service_entry:
    sub sp, sp, #0x20
    stp x19, x20, [sp]
    stp x21, x30, [sp, #0x10]
    mov x1, sp
    bl training_mod_native_service_dispatch
    ldr x30, [sp, #0x18]
    add sp, sp, #0x20
    ret
.size training_mod_native_service_entry, .-training_mod_native_service_entry
