//! The one native650010 function entry. Capture dispatcher registers before Rust,
//! run plugin pre-service work, then preserve the existing native call/refusal path.
use std::sync::atomic::{AtomicUsize, Ordering};

/// Numeric entry identities only. Borrowed for this callback, never retained as
/// native pointer authority. Private words are actual x19/x20/x21/LR, in that
/// order, from native_service.S; no Rust constructor or InlineCtx conversion.
#[repr(C)]
pub(crate) struct ServiceEntryContext {
    words: [usize; 4],
}
impl ServiceEntryContext {
    pub(super) fn caller_lr(&self) -> usize {
        self.words[3]
    }
    #[cfg(feature = "console-swap-trace")]
    fn native_sp(&self) -> usize {
        // native_service.S places these four words at original SP - 0x20.
        // Capture the actual callback stack; TLS alone cannot identify a fiber stack.
        self as *const Self as usize + std::mem::size_of::<Self>()
    }
    #[cfg(any(feature = "source-capacity-probe", feature = "managed-third-source-alpha"))]
    pub(super) fn dispatcher_tuple(&self) -> [usize; 4] {
        self.words
    }
}

const _: () = assert!(std::mem::size_of::<ServiceEntryContext>() == 0x20);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
core::arch::global_asm!(include_str!("native_service.S"));
extern "C" {
    fn training_mod_native_service_entry();
}

/// Startup only, after character_roster's existing pristine instruction checks
/// and before game callbacks. No second650010 hook is installed.
pub(super) unsafe fn install(base: usize) {
    let mut original = std::ptr::null_mut();
    skyline::hooks::A64HookFunction(
        (base + 0x650010) as *const _,
        training_mod_native_service_entry as *const _,
        &mut original,
    );
    assert!(!original.is_null(), "native service trampoline unavailable");
    ORIGINAL.store(original as usize, Ordering::Release);
}

/// Native functor651640 passes only FE in x0 and tail-branches to650010. The
/// assembly entry supplies the actual pre-Rust context in x1. This is a C ABI
/// function boundary, unlike the mid-function ordinary publication gate.
#[no_mangle]
unsafe extern "C" fn training_mod_native_service_dispatch(
    entry: *mut u8,
    context: &ServiceEntryContext,
) -> u64 {
    #[cfg(feature = "console-swap-trace")]
    let native_sp = context.native_sp() as u64;
    #[cfg(not(feature = "console-swap-trace"))]
    let native_sp = 0;
    crate::practice_trace::event(220, [entry as u64, context.caller_lr() as u64, native_sp, 0, 0]);
    // A plugin consumer returning early ends only pre-dispatch, never native work.
    super::service(context, entry);
    crate::practice_trace::event(221, [entry as u64, 0, 0, 0, 0]);
    #[cfg(feature = "mii-ordinary-player-alpha")]
    if !super::mii_ordinary_player::enter_native(entry) {
        return 0;
    }
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    if !super::mii_ordinary_cpu::enter_native(entry) {return 0;}
    #[cfg(feature="mii-managed-helper-alpha")]
    if !super::mii_managed_helper::enter_native(entry) {return 0;}
    let original: unsafe extern "C" fn(*mut u8) -> u64 =
        std::mem::transmute(ORIGINAL.load(Ordering::Acquire));
    let result = original(entry);
    crate::practice_trace::event(222, [entry as u64, result, 0, 0, 0]);
    #[cfg(feature="mii-managed-helper-alpha")]
    super::mii_managed_helper::leave_native(entry);
    #[cfg(feature = "mii-ordinary-player-alpha")]
    super::mii_ordinary_player::leave_native(entry);
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    super::mii_ordinary_cpu::leave_native(entry);
    crate::practice_trace::event(223, [entry as u64, result, 0, 0, 0]);
    result
}
