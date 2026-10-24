//! Shared native saved-original primitives. No hook, request, file IO or diagnostic dispatch.
//! The caller establishes current native service/INSTALL authority. Stored integers are identities.
#[cfg(any(feature = "mii-cpu-adapter-probe", feature = "mii-saved-alpha"))]
use skyline::hooks::{getRegionAddress, Region};
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
use training_mod_consts::mii::lifetime::Binding;
use training_mod_consts::mii::lifetime::Tokens as ValueTokens;
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
use training_mod_consts::mii::native_values::{self as values, RecordUpdate};
#[cfg(feature = "mii-saved-alpha")]
pub(crate) const CHARINFO_LEN: usize = 88;
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) const POOL_HOLDER: usize = 0x593b6a0;
pub(crate) const OWNER_TABLE: usize = 0x5324680;
pub(crate) const UI_DB: usize = 0x532f730;
pub(crate) const OUTFIT_TABLE: usize = 0x248;
pub(crate) const HAT_TABLE: usize = 0x238;
pub(crate) const OUTFIT_ROW: usize = 0x3282b70;
pub(crate) const HAT_ROW: usize = 0x32870b0;
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) const LOCK: usize = 0x39c19c0;
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) const UNLOCK: usize = 0x39c19d0;
pub(crate) unsafe fn rd<T: Copy>(p: usize, o: usize) -> T {
    std::ptr::read_volatile((p + o) as *const T)
}
pub(crate) unsafe fn wr<T: Copy>(p: usize, o: usize, v: T) {
    std::ptr::write_volatile((p + o) as *mut T, v);
}
pub(crate) fn aligned(p: usize) -> bool {
    p != 0 && p.is_multiple_of(8)
}
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub(crate) binding: Binding,
    pub(crate) thread: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub outfit: u16,
    pub hat: u16,
    pub colour: u16,
    pub voice: u16,
    pub specials: [u8; 4],
}

impl Tokens {
    pub unsafe fn from_row(e: usize) -> Self {
        Self {
            outfit: rd(e, 0x1aa),
            hat: rd(e, 0x1ac),
            colour: rd(e, 0x1ae),
            voice: rd(e, 0x1b0),
            specials: [rd(e, 0x1b2), rd(e, 0x1b3), rd(e, 0x1b4), rd(e, 0x1b5)],
        }
    }
}

/// Live write targets, all derived from the current callback argument on the current shot.
pub struct Targets {
    pub e: usize,
    pub fe: usize,
    pub d: usize,
    pub o: usize,
}

/// Writes exactly: `u16 E+0x1aa, E+0x1ae, E+0x1b0`, `u8 E+0x1b2..0x1b5`; `u16 FE+0x48, FE+0x4c, FE+0x4e`;
/// `u16 D+0x74, D+0x78`, `u32 D+0x390..0x39c`; `u32 O+0x6c, O+0x74, O+0x78`. Hat fields
/// (`E+0x1ac`, `FE+0x4a`, `D+0x76`, `O+0x70`), record indices and `D+0x7a` are never written.
pub unsafe fn write_tokens(t: &Targets, tokens: Tokens) {
    wr::<u16>(t.e, 0x1aa, tokens.outfit);
    wr::<u16>(t.e, 0x1ae, tokens.colour);
    wr::<u16>(t.e, 0x1b0, tokens.voice);
    for (i, s) in tokens.specials.iter().enumerate() {
        wr::<u8>(t.e, 0x1b2 + i, *s);
    }
    wr::<u16>(t.fe, 0x48, tokens.outfit);
    wr::<u16>(t.fe, 0x4c, tokens.colour);
    wr::<u16>(t.fe, 0x4e, tokens.voice);
    wr::<u16>(t.d, 0x74, tokens.outfit);
    wr::<u16>(t.d, 0x78, tokens.colour);
    for (i, s) in tokens.specials.iter().enumerate() {
        wr::<u32>(t.d, 0x390 + i * 4, u32::from(*s));
    }
    wr::<u32>(t.o, 0x6c, u32::from(tokens.outfit));
    wr::<u32>(t.o, 0x74, u32::from(tokens.colour));
    wr::<u32>(t.o, 0x78, u32::from(tokens.voice));
}

pub(crate) unsafe fn owner_object(base: usize, id: u32) -> Option<usize> {
    if id >= 8 {
        return None;
    }
    let table: usize = rd(base, OWNER_TABLE);
    if !aligned(table) {
        return None;
    }
    let o: usize = rd(table, 0xe8 + id as usize * 8);
    aligned(o).then_some(o)
}

pub(crate) unsafe fn lookup_row_id(
    base: usize,
    table_off: usize,
    hint: u16,
) -> Result<u64, &'static str> {
    let getter = match table_off {
        OUTFIT_TABLE => OUTFIT_ROW,
        HAT_TABLE => HAT_ROW,
        _ => return Err("unsupported ui table"),
    };
    let holder: usize = rd(base, UI_DB);
    if !aligned(holder) {
        return Err("ui db holder null/misaligned");
    }
    let db: usize = rd(holder, 8);
    if !aligned(db) {
        return Err("ui db null/misaligned");
    }
    let table: usize = rd(db, table_off);
    if !aligned(table) {
        return Err("ui table null/misaligned");
    }
    let data: usize = rd(table, 8);
    if !aligned(data) {
        return Err("ui table data null/misaligned");
    }
    let header: usize = rd(data, 0x20);
    if header == 0 || rd::<u8>(header, 0) != 0xc {
        return Err("ui table header type is not 0xc");
    }
    let arrays: usize = rd(data, 0x18);
    if !aligned(arrays) {
        return Err("ui table arrays null/misaligned");
    }
    if !aligned(rd::<usize>(arrays, 0x20)) {
        return Err("ui table keys null/misaligned");
    }
    if rd::<usize>(arrays, 0x28) == 0 {
        return Err("ui table offset array null");
    }
    let lookup: unsafe extern "C" fn(usize, u32) -> u64 = std::mem::transmute(base + getter);
    Ok(lookup(data, u32::from(hint)))
}

/// Raw rederivation only: no manager lookup, native callback, logging, allocation or Rust mutex.
/// With locked_g=Some, verify the current holder still names that mutex before reading its vector.
#[cfg(feature = "mii-native-adapter-probe")]
pub(crate) unsafe fn context(
    base: usize,
    fe: usize,
    e: usize,
    locked_g: Option<usize>,
    thread: usize,
    owner: u32,
    epoch: u64,
) -> Result<Context, &'static str> {
    context_for_kind(base, fe, e, locked_g, thread, (owner, epoch), 0x49)
}

/// Class comes from the current saved-row binding; never changes the native kind/index.
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) unsafe fn context_for_kind(
    base: usize, fe: usize, e: usize, locked_g: Option<usize>,
    thread: usize, owner_epoch: (u32, u64), kind: u32,
) -> Result<Context, &'static str> {
    let (owner, epoch) = owner_epoch;
    let holder: usize = rd(base, POOL_HOLDER);
    if !aligned(holder) {
        return Err("adapter: holder null/misaligned");
    }
    let g: usize = rd(holder, 0);
    if !aligned(g) || locked_g.is_some_and(|locked| g != locked) {
        return Err("adapter: manager changed or null/misaligned");
    }
    if rd::<u32>(fe, 0xe0) != owner
        || !(0x48..=0x4a).contains(&kind)
        || rd::<u32>(fe, 0x18) != kind
        || i64::from(rd::<i8>(e, 8)) != i64::from(kind)
        || rd::<u32>(e, 0x14) != owner
        || rd::<u16>(e, 0x1a8) != rd::<u16>(fe, 0x46)
    {
        return Err("adapter: current row/entry identity changed");
    }
    let binding = Binding {
        // Caller owns the generation; diagnostic wrappers deliberately use zero.
        epoch,
        owner,
        entry: fe,
        row: e,
        holder,
        manager: g,
        vector_begin: rd(g, 0x24d0),
        vector_end: rd(g, 0x24d8),
        record_index: rd(e, 0x1a8),
    };
    values::record_address(binding, binding).map_err(|_| "adapter: invalid vector/index")?;
    Ok(Context { binding, thread })
}

#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) fn tokens(t: Tokens) -> ValueTokens {
    ValueTokens {
        outfit: t.outfit,
        hat: t.hat,
        colour: t.colour,
        voice: t.voice,
        specials: t.specials,
    }
}
impl From<ValueTokens> for Tokens {
    fn from(t: ValueTokens) -> Self {
        Self {
            outfit: t.outfit,
            hat: t.hat,
            colour: t.colour,
            voice: t.voice,
            specials: t.specials,
        }
    }
}
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) struct PoolLock {
    pub(crate) g: usize,
    unlock: unsafe extern "C" fn(usize),
}
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
impl PoolLock {
    pub(crate) unsafe fn acquire(base: usize, g: usize) -> Self {
        let lock: unsafe extern "C" fn(usize) = std::mem::transmute(base + LOCK);
        let unlock = std::mem::transmute::<usize, unsafe extern "C" fn(usize)>(base + UNLOCK);
        lock(g);
        Self { g, unlock }
    }
}
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
impl Drop for PoolLock {
    fn drop(&mut self) {
        unsafe { (self.unlock)(self.g) }
    }
}
/// The single CharInfo write primitive used by diagnostic and alpha adapters. All validation
/// must precede this call. Native shared_ptr bytes +58..68 are deliberately untouched.
#[cfg(any(feature = "mii-native-adapter-probe", feature = "mii-saved-alpha"))]
pub(crate) unsafe fn copy_update(record: usize, update: &RecordUpdate) {
    for (offset, byte) in update.charinfo().iter().enumerate() {
        wr(record, offset, *byte);
    }
}
#[cfg(any(feature = "mii-cpu-adapter-probe", feature = "mii-saved-alpha"))]
#[path = "mii_native_cpu_scope.rs"]
pub(crate) mod cpu;
