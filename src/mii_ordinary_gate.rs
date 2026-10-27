//! Exact-site A64HookFunction gate; this is deliberately NOT an inline callback.
use std::sync::atomic::{AtomicUsize,Ordering};
static CONTINUE:AtomicUsize=AtomicUsize::new(0);
static REFUSE:AtomicUsize=AtomicUsize::new(0);
core::arch::global_asm!(include_str!("mii_ordinary_gate.S"));
extern "C" {fn mii_ordinary_gate_entry();}
const CODE:&[(usize,usize,u64)]=&[
    (0x650010,0x1650,0xcd26bcb400e7c302),
    (0x6556b0,0x850,0xcd8a4532b8e1f9f3),
    (0x652160,0x1b0,0x6eeaa7f5af9329d9),
    (0x651464,0x2c,0xba1c3d544b1b7941),
    (0x44be0d4,0x30,0x3ae3e04231cc15d2),
    (0x44be248,0x24,0xa605859e5cafaac7),
 ];
pub(super) unsafe fn valid(base:usize)->bool {
    CODE.iter().all(|(at,n,want)| {
        std::slice::from_raw_parts((base+at) as *const u8,*n).iter()
            .fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3))==*want
    })
}
/// Separate opt-in qualification: equal-hat loaders retain their existing gate.
/// 13.0.5 ELF SHA256 0789f0a4ae9652f335eca5eabc7df3e58b0954012a0bebdea374f317706536c2.
/// These sites are outside the detour patch; recheck before INSTALL and publication.
pub(super) unsafe fn headgear_valid(base:usize)->bool {
    const WORDS:&[(usize,u32)]=&[
        (0x650910,0x7940966a), // FE+4a -> cached hat
        (0x651358,0xa906a6a8), // cached tokens -> O+68..77
        (0x66dd50,0x79435a89), // E+1ac -> hat
        (0x66ddfc,0x290da688), // outfit/hat -> O+6c/+70
        (0x17e6c34,0xb9407298), // requester reads O+70
        (0x17e6cb8,0x34002698), // zero hat skips request
        (0x17e6eb8,0x11000ef9), // kind*42 + 3
        (0x17e6ed4,0x97ffefc7), // missing-resource attach
        (0x17e70ec,0x97ffef41), // resolved-resource attach
        (0x17e46d0,0x97fff8e8), // reset destroys resource tree
        (0x17e46d8,0xa9007e7f), // reset empties tree
        (0x17e46e4,0xf803c27f), // reset clears configuration latch
    ];
    WORDS.iter().all(|(at,want)|std::ptr::read_volatile((base+at) as *const u32)==*want)
}
pub(super) unsafe fn install(base:usize) {
    // Startup only, before any game callback can execute. Engine publishes original before patch.
    REFUSE.store(base+0x651464,Ordering::Release);
    let mut original=std::ptr::null_mut();
    skyline::hooks::A64HookFunction((base+0x6510f4) as *const _,
        mii_ordinary_gate_entry as *const _, &mut original);
    assert!(!original.is_null(),"ordinary Player gate trampoline unavailable");
    CONTINUE.store(original as usize,Ordering::Release);
}
#[no_mangle]
unsafe extern "C" fn mii_ordinary_gate_decide(entry:usize,selected:usize,native_sp:usize)->usize {
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if super::super::mii_ordinary_cpu::tracked(entry) {
        let valid_stack=native_sp.is_multiple_of(16) && super::native::span(native_sp,0xf0,false);
        return if valid_stack && super::super::mii_ordinary_cpu::prequeue(entry,selected) {
            CONTINUE.load(Ordering::Acquire)
        } else {
            super::super::mii_ordinary_cpu::refuse(entry,"prequeue refused; native epilogue selected");
            REFUSE.load(Ordering::Acquire)
        };
    }
    #[cfg(feature="mii-managed-helper-alpha")]
    if super::super::mii_managed_helper::tracked(entry) {
        let valid_stack=native_sp.is_multiple_of(16) && super::native::span(native_sp,0xf0,false);
        return if valid_stack && super::super::mii_managed_helper::prequeue(entry,selected) {
            CONTINUE.load(Ordering::Acquire)
        } else {
            super::super::mii_managed_helper::refuse(entry,"prequeue refused; native epilogue selected");
            REFUSE.load(Ordering::Acquire)
        };
    }
    if !super::tracked(entry) {return CONTINUE.load(Ordering::Acquire);}
    let valid_stack=native_sp.is_multiple_of(16) && super::native::span(native_sp,0xf0,false);
    if valid_stack && super::prequeue(entry,selected) {
        CONTINUE.load(Ordering::Acquire)
    } else {
        super::refuse(entry,"prequeue refused; native epilogue selected");
        REFUSE.load(Ordering::Acquire)
    }
}
