//! Chain the four shared EffectModule entries without relocating a far stub's
//! embedded handler address as instructions. Startup only; no per-frame work.

fn far_handler(words: [u32; 4]) -> Option<usize> {
    (words[..2] == [0x5800_0051, 0xd61f_0220])
        .then_some((u64::from(words[2]) | (u64::from(words[3]) << 32)) as usize)
}

#[derive(Debug, PartialEq, Eq)]
enum Plan { Native, Chain(usize), Reject }

fn plan(words: [u32; 4], native: [u32; 4], site: usize, replacement: usize,
        executable: impl FnOnce(usize) -> bool) -> Plan {
    if words == native { return Plan::Native; }
    match far_handler(words) {
        Some(previous) if previous != 0 && previous.is_multiple_of(4)
            && previous != replacement && !(site..site + 16).contains(&previous)
            && executable(previous) => Plan::Chain(previous),
        _ => Plan::Reject,
    }
}

#[cfg(target_arch = "aarch64")]
mod runtime {
    use super::*;
    use std::sync::OnceLock;

    #[repr(C)]
    #[derive(Default)]
    struct MemoryInfo {
        address: u64, size: u64, kind: u32, attributes: u32,
        permission: u32, ipc: u32, device: u32, padding: u32,
    }

    unsafe fn accessible(address: usize, size: usize, permissions: u32) -> bool {
        let Some(end) = address.checked_add(size) else { return false; };
        let mut info = MemoryInfo::default();
        let result: u64;
        core::arch::asm!("svc 0x6", inout("x0") &mut info as *mut MemoryInfo as u64 => result,
            lateout("x1") _, in("x2") address, clobber_abi("C"), options(nostack));
        result as u32 == 0 && info.permission & permissions == permissions
            && info.address <= address as u64
            && info.address.checked_add(info.size).is_some_and(|limit| end as u64 <= limit)
    }

    unsafe fn words(site: usize) -> [u32; 4] {
        std::array::from_fn(|n| ((site + n * 4) as *const u32).read_volatile())
    }

    unsafe fn points_to(site: usize, replacement: usize) -> bool {
        let current = words(site);
        if far_handler(current) == Some(replacement) { return true; }
        // Skyline uses one B instruction if the new handler is within reach.
        current[0] & 0xfc00_0000 == 0x1400_0000
            && site.wrapping_add_signed(((current[0] as i32) << 6 >> 4) as isize) == replacement
    }

    pub unsafe fn install(label: &str, site: usize, replacement: usize,
                          original: &OnceLock<u64>, native: [u32; 4],
                          ordinary_install: impl FnOnce()) {
        if original.get().is_some() { return; }
        if !site.is_multiple_of(8) || !accessible(site, 16, 5)
            || !accessible(replacement, 4, 5) {
            crate::logging::diagnostic(format_args!("[EffectHookChain] {label}: inaccessible entry; left unchanged"));
            return;
        }
        let before = words(site);
        let next = plan(before, native, site, replacement, |p| accessible(p, 4, 5));
        crate::logging::diagnostic(format_args!(
            "[EffectHookChain] {label}: site={site:#x} words={before:x?} plan={next:?}"));
        match next {
            Plan::Native => ordinary_install(),
            Plan::Chain(previous) => {
                // original!() is ready before the new callback becomes reachable.
                if original.set(previous as u64).is_err() { return; }
                skyline::hooks::A64HookFunction(site as *const _, replacement as *const _,
                                              std::ptr::null_mut());
            }
            Plan::Reject => return,
        }
        let installed = points_to(site, replacement);
        crate::logging::diagnostic(format_args!("[EffectHookChain] {label}: installed={installed}"));
    }
}

#[cfg(target_arch = "aarch64")]
pub use runtime::install;

#[cfg(test)]
mod tests {
    use super::*;
    const NATIVE: [u32; 4] = [0xf940a000, 0xf9400008, 0xf9413105, 0x12000042];
    const SITE: usize = 0x65ff41bbb0;
    const REPLACEMENT: usize = 0x30e721c828;

    #[test]
    fn physical_crash_pointer_is_chained_not_relocated() {
        let handler = 0x1dc9fa678;
        let stub = [0x58000051, 0xd61f0220, 0xdc9fa678, 1];
        assert_eq!(plan(stub, NATIVE, SITE, REPLACEMENT, |p| p == handler), Plan::Chain(handler));
        // The previous relocator decoded this pointer word with a negative
        // displacement, then added its zero-extended value to the third PC.
        let displacement = ((stub[2] as i32) << 8 >> 11) & !3;
        assert_eq!((SITE + 8).wrapping_add(displacement as u32 as usize), 0x66ff35b084);
        assert_eq!((SITE + 8).wrapping_add_signed(displacement as isize), 0x65ff35b084);
    }

    #[test]
    fn native_and_invalid_chains_are_distinguished() {
        assert_eq!(plan(NATIVE, NATIVE, SITE, REPLACEMENT, |_| panic!("not a chain")), Plan::Native);
        for handler in [0, SITE, SITE + 8, REPLACEMENT, 3] {
            let stub = [0x58000051, 0xd61f0220, handler as u32, (handler >> 32) as u32];
            assert_eq!(plan(stub, NATIVE, SITE, REPLACEMENT, |_| true), Plan::Reject);
        }
        assert_eq!(plan([0x58000051,0xd61f0220,0xdc9fa678,1], NATIVE, SITE, REPLACEMENT, |_| false), Plan::Reject);
        assert_eq!(plan([0x58000051,0,0xdc9fa678,1], NATIVE, SITE, REPLACEMENT, |_| true), Plan::Reject);
    }
}
