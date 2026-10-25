//! Narrow, callback-scoped Lua operations for the owned native Mii UI.
//! Lua errors stay inside the game's protected loader/caller. No Rust callback
//! is entered from Lua, and no native pointer enters a persisted UI snapshot.
use super::native::{aligned, read, write};
use std::{convert::TryFrom, mem::{size_of, transmute}};

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Value { bits: u64, tag: i32, padding: u32 }

impl Value {
    const NIL: Self = Self { bits: 0, tag: 0, padding: 0 };
    pub(super) fn integer(value: i32) -> Self {
        Self { bits: value as i64 as u64, tag: 0x13, padding: 0 }
    }
    pub(super) fn int(self) -> Option<i32> {
        match self.tag {
            0x13 => i32::try_from(self.bits as i64).ok(),
            0x03 => {
                let value = f32::from_bits(self.bits as u32);
                // All fields here are small integer ids. Reject truncation,
                // NaN and infinities instead of accepting a damaged snapshot.
                if value.is_finite() && value.fract() == 0.0
                    && (-4096.0..=4096.0).contains(&value) { Some(value as i32) }
                else { None }
            }
            _ => None,
        }
    }
}

const _: () = assert!(size_of::<Value>() == 16);

#[derive(Debug)]
pub(super) enum Error { Binding, Stack, Load(i32), Call(i32), Identity, Value }
impl Error {
    pub(super) fn code(&self) -> i32 {
        match *self {
            Self::Binding => 1, Self::Stack => 2, Self::Identity => 3, Self::Value => 4,
            Self::Load(status) => 0x100 + status,
            Self::Call(status) => 0x200 + status,
        }
    }
}

#[repr(C)]
struct Reader { data: *const u8, len: usize }

pub(super) struct Specials {
    pub sf: Value,
    pub pending: Value,
    pub main: Value,
    pub sub: Value,
}

pub(super) struct Vm {
    base: usize,
    state: usize,
    // Stack growth can relocate its allocation during load or protected calls.
    // Retain an offset only, never a saved raw stack pointer.
    saved_top_offset: usize,
}

impl Vm {
    /// Caller has rebound the live native component on its update thread, while
    /// the UI coroutine is suspended. This object cannot outlive that callback.
    pub(super) unsafe fn bind(base: usize, component: usize) -> Result<Self, Error> {
        // The native age-0 update creates the coroutine and age 1 first
        // resumes it. Before that resume the pending-choice table is empty.
        Self::bind_from(base, component, 2)
    }

    /// The Settings policy binds from age 1: `initialize` has built both
    /// selectors and `main` has not enabled input. Capture/restore keep `bind`.
    pub(super) unsafe fn bind_from(base: usize, component: usize, minimum_age: u32) -> Result<Self, Error> {
        if !aligned(component) || read::<i32>(component, 0xe0) != 1
            || read::<u32>(component, 0xe4) < minimum_age { return Err(Error::Binding); }
        Self::attach(base, component)
    }

    /// Headgear and Outfit leave `+0xe0` and `+0xe4` at 0 while the page is
    /// open. `+0xe8 == 1` and decide byte `+0x168 == 0` are that gate. The
    /// holder at `+0x40` is the same player chain Voice uses.
    pub(super) unsafe fn bind_gear(base: usize, component: usize) -> Result<Self, Error> {
        if !aligned(component) || read::<u32>(component, 0xe8) != 1
            || read::<u8>(component, 0x168) != 0 { return Err(Error::Binding); }
        Self::attach(base, component)
    }

    unsafe fn attach(base: usize, component: usize) -> Result<Self, Error> {
        let holder: usize = read(component, 0x40);
        if !aligned(holder) { return Err(Error::Binding); }
        let player: usize = read(holder, 0);
        if !aligned(player) { return Err(Error::Binding); }
        let state: usize = read(player, 0);
        let coroutine: usize = read(player, 8);
        if !aligned(state) || !aligned(coroutine) || state == coroutine
            || read::<usize>(state, 0x20) != state + 0x60 { return Err(Error::Binding); }
        let global: usize = read(state, 0x18);
        if !aligned(global) || read::<usize>(global, 0xc8) != state { return Err(Error::Binding); }
        let stack: usize = read(state, 0x38);
        let top: usize = read(state, 0x10);
        if !aligned(stack) || !aligned(top) || top < stack
            || !(top - stack).is_multiple_of(16) { return Err(Error::Stack); }
        let vm = Self { base, state, saved_top_offset: top - stack };
        vm.room(12)?;
        Ok(vm)
    }

    unsafe fn reset(&self) {
        let stack: usize = read(self.state, 0x38);
        write(self.state, 0x10, stack + self.saved_top_offset);
    }

    unsafe fn room(&self, slots: usize) -> Result<usize, Error> {
        let ci: usize = read(self.state, 0x20);
        if ci != self.state + 0x60 { return Err(Error::Stack); }
        let top: usize = read(self.state, 0x10);
        let stack: usize = read(self.state, 0x38);
        let last: usize = read(self.state, 0x30);
        let frame_top: usize = read(ci, 8);
        let function: usize = read(ci, 0);
        let end = top.checked_add(slots.checked_mul(16).ok_or(Error::Stack)?).ok_or(Error::Stack)?;
        if !aligned(top) || !aligned(stack) || top < stack || top < function + 16
            || end > last || end > frame_top { return Err(Error::Stack); }
        Ok(top)
    }

    pub(super) unsafe fn call<const N: usize>(
        &mut self, chunk: &[u8], args: &[Value],
    ) -> Result<[Value; N], Error> {
        if N > 8 || args.len() > 8 { return Err(Error::Stack); }
        self.reset();
        self.room(12)?;
        let load: unsafe extern "C" fn(usize, usize, *mut Reader, *const u8, *const u8) -> i32 =
            transmute(self.base + 0x38f5fe0);
        let pcall: unsafe extern "C" fn(usize, i32, i32, i32, isize, usize) -> i32 =
            transmute(self.base + 0x38f5d80);
        let mut reader = Reader { data: chunk.as_ptr(), len: chunk.len() };
        let loaded = load(self.state, self.base + 0x38f88f0, &mut reader,
            b"=mii_resume\0".as_ptr(), b"b\0".as_ptr());
        if loaded != 0 { self.reset(); return Err(Error::Load(loaded)); }
        // Re-read every stack address after a call that may grow it.
        let top = match self.room(args.len().max(N)) {
            Ok(top) => top,
            Err(error) => { self.reset(); return Err(error); }
        };
        for (index, value) in args.iter().enumerate() { write(top, index * 16, *value); }
        write(self.state, 0x10, top + args.len() * 16);
        let called = pcall(self.state, args.len() as i32, N as i32, 0, 0, 0);
        if called != 0 { self.reset(); return Err(Error::Call(called)); }
        let top: usize = read(self.state, 0x10);
        let stack: usize = read(self.state, 0x38);
        let expected = stack + self.saved_top_offset + N * 16;
        if top != expected { self.reset(); return Err(Error::Stack); }
        let mut results = [Value::NIL; N];
        for (index, value) in results.iter_mut().enumerate() {
            *value = read(stack, self.saved_top_offset + index * 16);
        }
        self.reset();
        Ok(results)
    }

    pub(super) unsafe fn specials(&mut self) -> Result<Specials, Error> {
        let [main, initialize] = self.call::<2>(include_bytes!("lua_helpers/closures.luac"), &[])?;
        Self::check_closure(main, 6, 46)?;
        Self::check_closure(initialize, 16, 180)?;
        let result = Specials {
            sf: Self::upvalue(main, 1)?,
            pending: Self::upvalue(main, 3)?,
            main: Self::upvalue(main, 4)?,
            sub: Self::upvalue(initialize, 2)?,
        };
        if [result.sf, result.pending, result.main, result.sub].iter().any(|value| value.tag != 0x45) {
            return Err(Error::Identity);
        }
        Ok(result)
    }

    pub(super) unsafe fn settings(&mut self) -> Result<Value, Error> {
        // Qualified Settings chunk: main and initialize share upvalue 0,
        // the table owning selector0 (rows 0..5) and selector1 (Random/Test).
        let [main, initialize] = self.call::<2>(include_bytes!("lua_helpers/closures.luac"), &[])?;
        Self::check_closure(main, 4, 107)?;
        Self::check_closure(initialize, 10, 225)?;
        let settings = Self::upvalue(main, 0)?;
        let initialized = Self::upvalue(initialize, 0)?;
        if settings.tag != 0x45 || initialized.tag != 0x45 || settings.bits != initialized.bits {
            return Err(Error::Identity);
        }
        Ok(settings)
    }

    /// Voice chunk from the 13.0.5 listing `private-ui-5c0411b58dad0582`:
    /// main is 5 upvalues / 41 instructions, initialize is 17 / 255, and both
    /// close over the same page table (selector, selected_id, voice_type, voice_pitch).
    pub(super) unsafe fn voice_page(&mut self) -> Result<Value, Error> {
        let [main, initialize] = self.call::<2>(include_bytes!("lua_helpers/closures.luac"), &[])?;
        Self::check_closure(main, 5, 41)?;
        Self::check_closure(initialize, 17, 255)?;
        let page = Self::upvalue(main, 0)?;
        let initialized = Self::upvalue(initialize, 0)?;
        if page.tag != 0x45 || initialized.tag != 0x45 || page.bits != initialized.bits {
            return Err(Error::Identity);
        }
        Ok(page)
    }

    /// Headgear and Outfit yield inside a 2-upvalue, 28-instruction frame.
    /// Upvalue 0 is the environment. Upvalue 1 is the page table. The yielded
    /// function is the light C at ELF `0x39034d0`; its caller link is `+0x10`.
    pub(super) unsafe fn gear_page(component: usize) -> Result<Value, Error> {
        let holder: usize = read(component, 0x40);
        if !aligned(holder) { return Err(Error::Binding); }
        let player: usize = read(holder, 0);
        if !aligned(player) { return Err(Error::Binding); }
        let coroutine: usize = read(player, 8);
        if !aligned(coroutine) { return Err(Error::Binding); }
        let ci: usize = read(coroutine, 0x20);
        if !aligned(ci) { return Err(Error::Binding); }
        let current_slot: usize = read(ci, 0);
        if !aligned(current_slot) { return Err(Error::Binding); }
        let current: Value = read(current_slot, 0);
        if current.tag != 0x16 { return Err(Error::Binding); }
        let previous: usize = read(ci, 0x10);
        if !aligned(previous) { return Err(Error::Binding); }
        let slot: usize = read(previous, 0);
        if !aligned(slot) { return Err(Error::Binding); }
        let closure: Value = read(slot, 0);
        Self::check_closure(closure, 2, 28)?;
        let environment = Self::upvalue(closure, 0)?;
        let page = Self::upvalue(closure, 1)?;
        if environment.tag != 0x45 || page.tag != 0x45 { return Err(Error::Identity); }
        Ok(page)
    }

    unsafe fn check_closure(value: Value, upvalues: i32, instructions: i32) -> Result<(), Error> {
        if value.tag != 0x46 || !aligned(value.bits as usize) { return Err(Error::Identity); }
        let proto: usize = read(value.bits as usize, 0x18);
        if !aligned(proto) || read::<i32>(proto, 0x10) != upvalues
            || read::<i32>(proto, 0x18) != instructions { return Err(Error::Identity); }
        Ok(())
    }

    unsafe fn upvalue(closure: Value, index: usize) -> Result<Value, Error> {
        let upvalue: usize = read(closure.bits as usize, 0x20 + index * 8);
        if !aligned(upvalue) { return Err(Error::Identity); }
        let value: usize = read(upvalue, 0);
        if !aligned(value) { return Err(Error::Identity); }
        Ok(read(value, 0))
    }
}

impl Drop for Vm {
    fn drop(&mut self) { unsafe { self.reset(); } }
}
