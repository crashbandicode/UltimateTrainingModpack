//! Native Mii portraits for Training and local waiting practice (default-off).
//!
//! After an exact Mii reload of a visible owner (P1/CPU1), lease the parked managed helper
//! (row 2/3) exclusively with the configuration that was loaded, issue one native portrait
//! render for the helper row through the native source `S'`, and bind the registered
//! descriptor into the visible slot's bust pane with `141b8a0`. The live fighter is never
//! rendered. An inline hook at main+0x32c7404 keeps the native renderer from copying the
//! manager template over the helper's D block for exactly that render; every other render
//! passes unchanged. Two render-only register overrides select the native class camera
//! and draw preset while keeping the owned request/texture storage identity unchanged.
//! Policy and lifetime rules live in `mii_hud_portrait_policy`.
//! Design: artifacts/enhancements/mii-hud54-design/{DESIGN,ROOT-FLAG-RESPONSE}.md.
//! Run44 (mii-hud62-runtime01) proved the Mii-origin Training path: saved Sword -> saved
//! Gunner rendered on helper 2 and bound, helper proof held, original restored, quit/re-entry.
//! hud64: the native pane applier (`1418780/1418c80 -> 1b4ee40`) draws the bust only when
//! `slot+0x1a8 != 0` and `table[slot+0x1ac]` is registered, otherwise the static portrait by
//! key. Ordinary-origin slots require the part-chain admission described below. Both owners' renders
//! on one helper row register into the same `table[0x12+row]`, and the applier re-binds from
//! that entry on re-apply, so one bust row serves one owner at a time.
//! hud66/hud71: presentation targets are layout-aware. Training: a slot not yet in bust mode
//! (`+0x1a8 == 0`, an ordinary origin) whose part chain `141b8a0` dereferences is present is put
//! into bust mode (`+0x1a8 = 1`, original words saved and logged) and bound with the same
//! `141b8a0`. It then stays a Mii-mode slot for the rest of this HUD: a later non-Mii kind is
//! presented by the native event-0x1d path for Mii-mode slots (`1363bd4..1363bfc`), the path a
//! Mii-origin slot already takes (hud48 request11-bowser). Returning `+0x1a8` to 0 is not done:
//! `141b960` raised the bust node `+0x7b0`, no native site lowers it, and its prior alpha is not
//! readable through an established offset. Waiting room: CPU4's card pane takes the rendered descriptor
//! through `379e9c0`, the override the bust bind itself uses; the product's kind icon
//! assignment replaces it on return. A slot without the part chain or a waiting scene without
//! the render pipeline is refused once with its words logged.
use super::mii_hud_portrait_policy as policy;
use super::mii_backend::Layout;
use super::{
    active_body, aligned, mii_backend, mii_managed_helper, mii_saved_alpha, native_source,
    pointer_chain, read, scope, sources, state_for, PortraitIntent, Role, BASE, INSTALL,
};
use crate::source_lifetime::Holder;
// All native fields below are borrowed under the current engine service callback
// or the exact render hook. Nullable parents use the centralized chain walker;
// required fields use its unchecked volatile primitive only after admission.
// Stored addresses are identities, rederived before use, never Rust ownership.
use crate::mii_native_access::PoolLock;
use skyline::hooks::InlineCtx;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use training_mod_consts::mii::lifetime::Configuration;

/// Verified four-word windows (13.0.5 main). Twenty from mii-hud48, six added by hud54/hud57
/// (`32c73f0/32c7400/32c7410` hook site, `13bb33c/13bb4a8` HUD record fields, `3304904` batch
/// teardown), three waiting-pane windows, ten portrait-setup, nine render-preset
/// and nine owned-pipeline retirement windows. Any mismatch
/// disables the feature and installs nothing.
const SITES: [(usize, [u32; 4]); 57] = [
    // Native source tick/select and reset. Only our drained waiting bootstrap
    // may enter cmd0; native code owns every descriptor destruction/free.
    (0x3308ef0, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xaa0003f3]),
    (0x3308f00, [0x97fff020, 0xf9400269, 0xb4000149, 0xa940ae68]),
    (0x3308f30, [0xb9413e68, 0x11000508, 0xb9013e68, 0xf0010128]),
    (0x3304f8c, [0xb9813408, 0x71004d1f, 0x54000562, 0x8b080808]),
    (0x3304ffc, [0x39451268, 0x340001a8, 0xa941aa68, 0x92400149]),
    (0x33021b0, [0xd10443ff, 0xa90e57fc, 0xa90f4ff4, 0xa9107bfd]),
    (0x33021c0, [0x910403fd, 0xb9413c08, 0x35001948, 0xd2dfe00a]),
    (0x3302490, [0xf9016008, 0x910b3008, 0xf9000109, 0x390b500a]),
    (0x33024d0, [0xb0010174, 0xf943ae93, 0xb40000d3, 0xaa1303e0]),
    // Existing Picture UV quad setter; writes 32 bytes, allocates and frees nothing.
    (0x445d0, [0xf9400408, 0xf940004a, 0x937b7c29, 0xf829690a]),
    (0x134373c, [0x9001fc08, 0xf9421108, 0xf9400508, 0xf9400100]),
    (0x13bb4d4, [0xf9406d08, 0xb9419902, 0x3100045f, 0x540000a0]),
    (0x13bb4e4, [0xf9405a68, 0xf94bc900, 0xaa1803e1, 0x940180ec]),
    (0x141b8a0, [0xa9bc5ff8, 0xa90157f6, 0xa9024ff4, 0xa9037bfd]),
    (0x141b8c4, [0x3946a128, 0x340008a8, 0x71007c5f, 0x54000868]),
    (0x141b904, [0xb88e8e96, 0x710022df, 0xb900c682, 0x3903068b]),
    (0x141b960, [0x1e2e1000, 0x911ec100, 0x948d7d5e, 0xf9407276]),
    (0x13437c4, [0xf9400108, 0x39403509, 0x340003e9, 0x529bfa09]),
    (0x13437f8, [0x9001ff69, 0xf9445929, 0xf9400529, 0x394ca12d]),
    (0x1343918, [0xb940014a, 0xb902e52b, 0xf001892b, 0x3dc3b160]),
    (0x1343928, [0xb902e12a, 0xb941352a, 0x3d80bd20, 0x7100455f]),
    (0x1343950, [0x8b2ac92a, 0x5280016b, 0xb900e94b, 0xb941352a]),
    (0x3304a04, [0xf900d67f, 0x97ff0bb2, 0xb4000175, 0x910022a8]),
    (0x3304b38, [0xb9000a68, 0x320003e8, 0xb9019a79, 0x39003a7f]),
    (0x3304b48, [0x3906727f, 0x79001a68, 0xf9400709, 0xf900e669]),
    (0x32c7248, [0xb9819a95, 0x71007ebf, 0x54000128, 0xf9400516]),
    (0x32c726c, [0xf8357ad7, 0x941be9d8, 0x320003e8, 0x39067288]),
    (0x32c7358, [0xd000ff89, 0xf9427d35, 0xf94002a9, 0x8b080d28]),
    (0x39c19c0, [0xd000c6f0, 0xf945e211, 0x912f0210, 0xd61f0220]),
    (0x39c19d0, [0xd000c6f0, 0xf945e611, 0x912f2210, 0xd61f0220]),
    (0x32c73f0, [0xb9800288, 0x7100211f, 0x54001522, 0xf94002b5]),
    (0x32c7400, [0x396c82a9, 0x340003a9, 0x8b080ea8, 0xf9401116]),
    (0x32c7410, [0xf9407ec8, 0x9105c2a1, 0x52801202, 0x9100a100]),
    (0x13bb33c, [0x8b1c027b, 0xb94d6378, 0x3100071f, 0x54000d60]),
    (0x13bb4a8, [0xb94d6b6a, 0x3100055f, 0x540000a1, 0xf94007e8]),
    (0x3304904, [0x97ff0c93, 0xaa1303e0, 0x9418a88d, 0xf903ae9f]),
    (0x379e9d0, [0x9100c3fd, 0x8b22d008, 0x91042117, 0x39442108]),
    (0x377aef0, [0x9100c3fd, 0xf9400408, 0xf9400513, 0xb4000813]),
    // Portrait-only native setup: no unrelated scene/fighter builder is admitted.
    (0x3302530, [0xb9413c08, 0xaa0003fb, 0x34002de8, 0xb0010168]),
    (0x3302540, [0xf943d108, 0xb4008088, 0xd0010095, 0x911362b5]),
    (0x3302af4, [0x394b5368, 0x34001448, 0xb0010173, 0xf943d268]),
    (0x3302d80, [0x394b4768, 0x34004308, 0xb0010168, 0xf943ad08]),
    (0x3303554, [0xb9413769, 0x71000528, 0x540000ab, 0x7100493f]),
    (0x3303a94, [0x320003e8, 0x390ca008, 0x394b5008, 0x34000468]),
    (0x3303b40, [0x90010168, 0xf943d108, 0xb40002c8, 0xf9400108]),
    (0x3303bd0, [0xb9413c08, 0x90010169, 0xf943d129, 0x7100011f]),
    (0x3303874, [0x90010168, 0xf943d108, 0xaa0003f3, 0xb4000ea8]),
    (0x3304fc0, [0x93407d08, 0x8b081268, 0xa942a109, 0xa900a269]),
    // Register-only native camera and draw preset overrides.
    (0x13438e4, [0x7101297f, 0x321e03ec, 0x1a8c158c, 0x7101257f]),
    (0x32c5cb4, [0xf9400d39, 0x38686ae8, 0xf960aae9, 0xb9800698]),
    (0x32c5cc4, [0xeb08013f, 0x54000de9, 0x8b080ef5, 0xf960b2a0]),
    (0x32c5f90, [0x321e0fe8, 0x9b086708, 0x2f00e401, 0x4ea11c20]),
    (0x32c66e8, [0x321e0fe8, 0x9b086708, 0x3d40310b, 0x4ea81d01]),
    (0x32c69fc, [0xb940068a, 0xf903a3e9, 0xf903a7f4, 0xf903b3f7]),
    (0x32c6a2c, [0x51001149, 0x7e21d860, 0x1e2a1800, 0xbd027fe0]),
    (0x32c6a5c, [0x71000d3f, 0x7e21d8e0, 0x1e2a1800, 0x90008d69]),
    (0x32c6e28, [0x2a1503e4, 0xaa1603e7, 0x9109c3f8, 0xd63f0100]),
];

const HUD_ROOT: [usize; 3] = [0x52c3420, 8, 0];
const SEQUENCE: [usize; 2] = [0x52c3760, 0];
const SOURCE: [usize; 2] = [0x532f8b0, 8];
const BATCH_HOLDER: usize = 0x532f758;
const NATIVE_SCENE_HOLDER: usize = 0x532f7a0;
const BATCH: [usize; 2] = [BATCH_HOLDER, 0];
const TABLE: [usize; 2] = [0x593bc38, 8];
const FIGHTERS: [usize; 2] = [0x52b94f8, 0];
const PLACEMENT: usize = 0x446aec0;
const SCENE_BYTE: usize = 0x52c51b0;
const REBIND: usize = 0x141b8a0;
/// `379e9c0(pane, descriptor, 0)`: the descriptor override `141b8a0` and `1b4ee40` apply.
const PANE_OVERRIDE: usize = 0x379e9c0;
const HOLDER_CATEGORY: u32 = 3;
const CHANNEL_BASE: usize = sources::PORTRAIT_CHANNELS[0];

const REFUSE_SCOPE: u32 = 1;
const REFUSE_LEASE: u32 = 2;
const REFUSE_RESTORE: u32 = 4;
const REFUSE_PANE: u32 = 8;
const REFUSE_HALT: u32 = 16;
const REFUSE_ROW: u32 = 32;
const REFUSE_PIPELINE: u32 = 64;
/// The slot is not in bust mode: every native reader (`141b8c4`, `1363bd4`, `1510e90`, `1418860`,
/// `1418ce0`) treats such a slot as having no bust; its construction writer is unidentified.
const NO_BUST: &str = "visible slot is not in bust mode (+0x1a8 == 0): unsupported";
/// The waiting scene has no portrait render pipeline objects (`S'`, `G`, batch, table).
const NO_PIPELINE: &str = "waiting scene has no portrait render pipeline: unsupported";
/// `141b8a0` aborts (`141b914` -> `141ba98`) or dereferences an absent part (`141b920..141b934`).
const NO_PART: &str = "visible slot has no bust part (+0xe8 or part chain): unsupported";

/// A Training slot this module bound although construction never bound it (`+0x1a9 == 0`),
/// with its original words (`+0x1a8` mode, `+0x1a9`, `+0x1ac`). Logged, not restored: the slot
/// stays in Mii mode for this HUD (see the module note). Cleared on scene change or teardown.
#[derive(Clone, Copy, Debug)]
struct Flipped { slot: i32, mode: u8, bound_flag: u8, index: u32 }
/// Pane/quad addresses are comparison identities only. Every write rederives the
/// live waiting card. Scene departure drops this value without touching old memory.
#[derive(Clone, Copy)]
struct WaitingCrop { pane: usize, coordinates: usize, original: [f32; 8], applied: [f32; 8] }
/// The other owner's bust is bound on this helper row; see `policy::row_free_for`.
const ROW_SHARED: &str = "helper row already shows the other owner's bust; one bust row per owner";

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Two native tick hooks return after this one load outside a departure. Kept
/// separate from ACTIVE so a departed in-flight render can still be retired.
static RETIRE_PIPELINE: AtomicBool = AtomicBool::new(false);
/// Jobs plus in-flight renders. Zero means `service` returns after one atomic load.
static PENDING: AtomicUsize = AtomicUsize::new(0);
static ARMED_REQUEST: AtomicUsize = AtomicUsize::new(0);
static ARMED_ENTRY: AtomicUsize = AtomicUsize::new(0);
static ARMED_ROW_SUB: AtomicU64 = AtomicU64::new(0);
static HOOK_HITS: AtomicU32 = AtomicU32::new(0);
static HOOK_SKIPS: AtomicU32 = AtomicU32::new(0);
static CAMERA_OVERRIDES: AtomicU32 = AtomicU32::new(0);
static DRAW_OVERRIDES: AtomicU32 = AtomicU32::new(0);
static STATE: Mutex<State> = Mutex::new(State::new());

/// The helper D mirrors the exact proof reads; captured before issue and after the drain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mirrors { index: u16, outfit: u16, hat: u16, colour: u16, rebuild: u8, specials: [u32; 4] }

#[derive(Clone, Copy, PartialEq, Eq)]
enum PipelinePhase { Live, Departed, Resetting }

/// Identities only, never cached borrows. Native Training destroys its source on
/// exit. Local-wireless waiting reuses it across a match (observed); Quickplay
/// waiting carries the same retirement duty until observed otherwise, so a
/// pipeline we started is returned to the unused image before native match setup.
/// A source that native code replaced instead is detected and forgotten. Existing
/// pipelines are never marked here. The exact waiting P1 entry is the departure authority.
#[derive(Clone, Copy)]
struct Bootstrap {
    source: usize, batch: usize, thread: usize, player_entry: usize,
    layout: Layout, kind: u32, phase: PipelinePhase, retirement_ticks: u16,
    thread_deferred: bool,
}

#[derive(Clone, Copy)]
// Exactly two fixed jobs plus their replacements; inline values avoid allocation in service.
#[allow(clippy::large_enum_variant)]
enum Intent { Exact(Configuration), Original { original_kind: u32 } }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage { Requested, Leased, Issued }

#[derive(Clone, Copy)]
struct Pending { entry: usize, kind: u32, generation: u64, logical: u32, physical: u32, layout: Layout, intent: Intent }

#[derive(Clone, Copy)]
struct Job {
    entry: usize, kind: u32, generation: u64, logical: u32, intent: Intent, stage: Stage,
    /// Physical owner (Training 0/1, waiting 0/4) and the layout the request was made in.
    physical: u32, layout: Layout,
    holder: Holder, lease: Option<sources::Lease>, helper: u32, helper_entry: usize,
    request: Option<policy::Request>, before: Option<Mirrors>, next: Option<Pending>,
    /// Issue-time numeric identities (compared, never dereferenced): `S'`, the batch object,
    /// and the hook counters before this job's render.
    source: usize, batch: usize, hits: u32, skips: u32, camera: u32, draw: u32,
    /// Last logged admission deferral, so each distinct reason is traced once per job.
    deferral: &'static str,
    /// Bounds setup deferral before any helper is leased (service callbacks, not wall time).
    pipeline_waits: u16,
}

struct State {
    jobs: [Option<Job>; policy::VISIBLE_OWNERS],
    own: policy::Ownership,
    batch: usize,
    refused: [u32; policy::VISIBLE_OWNERS],
    /// A render failed its after-render proof: its helper stays leased and no further
    /// portrait is admitted until scene departure.
    halted: bool,
    /// Training slots this module moved into bust mode, per logical owner.
    flipped: [Option<Flipped>; policy::VISIBLE_OWNERS],
    /// Retained through waiting departure until native retirement is confirmed.
    bootstrap: Option<Bootstrap>,
    waiting_crop: Option<WaitingCrop>,
}

impl State {
    const fn new() -> Self {
        Self { jobs: [None, None], own: policy::Ownership::new(), batch: 0, refused: [0, 0], halted: false, flipped: [None, None], bootstrap: None, waiting_crop: None }
    }
    fn sync_pending(&self) {
        PENDING.store(
            self.jobs.iter().flatten().count() + usize::from(self.own.in_flight.is_some()),
            Ordering::Release,
        );
    }
}

fn refuse_once(state: &mut State, owner: usize, bit: u32, reason: &str) {
    if state.refused[owner] & bit == 0 {
        state.refused[owner] |= bit;
        trace!("[MiiPortrait] owner={} refused: {}", owner, reason);
    }
}

fn new_job(p: Pending, owner: usize) -> Job {
    Job {
        entry: p.entry, kind: p.kind, generation: p.generation, logical: p.logical, intent: p.intent,
        physical: p.physical, layout: p.layout,
        stage: Stage::Requested,
        holder: Holder { category: HOLDER_CATEGORY, id: owner as u64 | (p.generation << 8) },
        lease: None, helper: u32::MAX, helper_entry: 0, request: None, before: None, next: None,
        source: 0, batch: 0, hits: 0, skips: 0, camera: 0, draw: 0, deferral: "", pipeline_waits: 0,
    }
}

fn arm(request: usize, entry: usize, row: u32, sub: u32) {
    ARMED_ROW_SUB.store((u64::from(row) << 32) | u64::from(sub), Ordering::Release);
    ARMED_ENTRY.store(entry, Ordering::Release);
    ARMED_REQUEST.store(request, Ordering::Release);
}

fn disarm() {
    ARMED_REQUEST.store(0, Ordering::Release);
    ARMED_ENTRY.store(0, Ordering::Release);
    ARMED_ROW_SUB.store(0, Ordering::Release);
}

/// Training and the qualified waiting rooms only (`policy::may_write_scene`);
/// matches, arenas and every other mode store nothing.
fn scene_writable() -> bool {
    match crate::common::game_mode::current() {
        Some(mode) => policy::may_write_scene(mode.melee, mode.mode),
        None => false,
    }
}

unsafe fn store<T: Copy>(base: *mut u8, offset: usize, value: T) {
    crate::native_fields::write_unchecked(base as usize, offset, value);
}

/// Startup only, after character_roster's pristine checks. Installs the scoped render hooks.
pub(super) unsafe fn init(base: usize) {
    for (offset, words) in SITES {
        if std::slice::from_raw_parts((base + offset) as *const u32, 4) != words {
            trace!("[MiiPortrait] fingerprint mismatch +0x{:x}; portrait disabled, no hook installed", offset);
            return;
        }
    }
    skyline::install_hooks!(template_skip, portrait_camera, portrait_draw, pipeline_before_tick, pipeline_after_tick);
    ACTIVE.store(true, Ordering::Release);
    trace!("[MiiPortrait] armed: helper-rendered portrait, scoped template skip at +0x32c7404, channels {:?}", sources::PORTRAIT_CHANNELS);
}

/// From `publish_mii_presentation`, after its exact-completion gates and `refresh_hud`.
/// `owner` is the physical owner (Training 0/1, waiting 0/4); jobs are kept per logical owner.
pub(super) unsafe fn request(entry: *const u8, owner: u32, generation: u64, kind: u32, intent: PortraitIntent) {
    if !ACTIVE.load(Ordering::Acquire) || !(0x48..=0x4a).contains(&kind) {
        return;
    }
    let intent = match intent {
        PortraitIntent::NotMii => return,
        PortraitIntent::Exact(c) => {
            if c.canonical().class as u32 != kind {
                trace!("[MiiPortrait] owner={} exact configuration class differs from loaded kind 0x{:x}; ignored", owner, kind);
                return;
            }
            Intent::Exact(c)
        }
        PortraitIntent::Original { original_kind } => Intent::Original { original_kind },
    };
    let Some(layout) = mii_backend::current() else { return; };
    let Some(logical) = layout.logical(owner) else { return; };
    let role = if logical == 0 { Role::Player } else { Role::Cpu };
    let in_scope = scope(role)
        .is_some_and(|s| s.owner == owner && (layout != Layout::Training || s.mode == 0x04040000));
    let pending = Pending { entry: entry as usize, kind, generation, logical: logical as u32, physical: owner, layout, intent };
    let o = logical;
    let mut released = None;
    {
        let mut s = STATE.lock().unwrap();
        if s.bootstrap.is_some_and(|p| p.phase != PipelinePhase::Live) { return; }
        if !in_scope {
            refuse_once(&mut s, o, REFUSE_SCOPE, "portrait scope is native Training or a qualified waiting room (local wireless, Quickplay); other scenes are unsupported");
            return;
        }
        if s.halted {
            refuse_once(&mut s, o, REFUSE_HALT, "portrait halted for this scene: an earlier render failed its after-render proof");
            return;
        }
        match s.jobs[o] {
            Some(job) if job.stage == Stage::Issued => {
                // The queued render keeps its lease and request until the native queue drains.
                s.own.supersede(o, generation);
                s.jobs[o] = Some(Job { next: Some(pending), ..job });
            }
            Some(job) => {
                released = job.lease;
                s.jobs[o] = Some(new_job(pending, o));
            }
            None => s.jobs[o] = Some(new_job(pending, o)),
        }
        s.sync_pending();
    }
    if let Some(lease) = released {
        sources::release(lease);
        sources::cancel(CHANNEL_BASE + o);
    }
}

/// From `character_roster::service`, every entry callback, before the Mii adapters.
pub(super) unsafe fn service(entry: *mut u8, owner: u32) {
    if !ACTIVE.load(Ordering::Acquire) || PENDING.load(Ordering::Acquire) == 0 {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    scene_tick(base);
    if STATE.lock().unwrap().own.in_flight.is_some() {
        drain_tick(base);
    }
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    if let Some(logical) = mii_backend::current().and_then(|l| l.logical(owner)) {
        job_tick(base, logical, entry as usize);
    }
}

/// Before native entry destruction. Visible owner: cancel intent, keep the pane record and
/// any queued render. Helper: the lease dies with the book's `native_destroyed`; a queued
/// render can no longer bind and is drained out.
pub(super) unsafe fn on_destroy(entry: *const u8, owner: u32) {
    if !ACTIVE.load(Ordering::Acquire) { return; }
    let departure = owner == 0 && STATE.lock().unwrap().bootstrap
        .is_some_and(|p| p.layout.waiting() && p.player_entry == entry as usize);
    if departure { request_pipeline_retirement(); }
    let mut released = None;
    {
        let mut s = STATE.lock().unwrap();
        for o in 0..policy::VISIBLE_OWNERS {
            let Some(job) = s.jobs[o] else { continue; };
            if job.entry == entry as usize {
                s.own.cancel_owner(o);
                if job.stage != Stage::Issued {
                    released = job.lease;
                    s.jobs[o] = None;
                } else {
                    // A dead owner never receives a queued successor.
                    s.jobs[o] = Some(Job { next: None, ..job });
                }
            } else if job.helper_entry == entry as usize && job.stage != Stage::Requested {
                s.own.depart();
                disarm();
                // The helper is freed after this callback: keep no address that could be read.
                s.jobs[o] = if job.stage == Stage::Issued { Some(Job { lease: None, helper_entry: 0, ..job }) } else { None };
            }
        }
        s.sync_pending();
    }
    if let Some(lease) = released {
        sources::release(lease);
    }
    if departure {
        // Same-thread fast path queues reset before native entry destruction. If
        // this caller differs from the captured service thread, the tick hooks
        // perform the handback; no native fields are touched on this thread.
        // Do not invoke the native tick/destructor here: the entry destructor's
        // caller may hold an engine lock also acquired by batch destruction.
        let base = BASE.load(Ordering::Acquire);
        if let Some(source) = pointer_chain(base as *const u8, &SOURCE) {
            begin_pipeline_retirement(base, source);
        }
    }
}

/// After `sources::scene_changed()`. Leases were invalidated there; a queued render is kept
/// (departed) until the native queue drains or the source object is gone. Nothing is reset
/// on pointer change alone; batch teardown is detected from its nulled holder.
pub(super) fn scene_changed() {
    if !ACTIVE.load(Ordering::Acquire) { return; }
    request_pipeline_retirement();
    let mut s = STATE.lock().unwrap();
    // The arm is kept: the hook only compares it with live registers and fresh globals. It is
    // cleared by the helper's own destructor, a batch change, or the end of the drain.
    s.own.depart();
    for o in 0..policy::VISIBLE_OWNERS {
        s.jobs[o] = match s.jobs[o] {
            Some(job) if job.stage == Stage::Issued => Some(Job { lease: None, next: None, helper_entry: 0, ..job }),
            _ => None,
        };
    }
    s.refused = [0; policy::VISIBLE_OWNERS];
    s.halted = false;
    s.flipped = [None; policy::VISIBLE_OWNERS];
    if s.bootstrap.is_none_or(|p| !p.layout.waiting()) {
        s.bootstrap = None;
        s.waiting_crop = None;
    }
    s.sync_pending();
}

unsafe fn scene_tick(base: usize) {
    let batch = read::<usize>(base as *const u8, BATCH_HOLDER);
    let mut s = STATE.lock().unwrap();
    if s.batch != batch {
        if s.batch != 0 {
            // Ownership here addresses only the current batch. Never reuse an old batch's
            // requests after replacement. This does not prove the issued source queue drained;
            // drain_tick separately checks its captured source identity before reading it.
            s.own.teardown();
            s.flipped = [None; policy::VISIBLE_OWNERS];
            disarm();
            trace!("[MiiPortrait] batch teardown observed old=0x{:x} new=0x{:x}; request/pane records cleared, queued render retained until drain", s.batch, batch);
        }
        s.batch = batch;
        s.sync_pending();
    }
}

unsafe fn job_tick(base: usize, owner: usize, entry: usize) {
    let Some(job) = STATE.lock().unwrap().jobs[owner] else { return; };
    if job.entry != entry {
        return;
    }
    if job.stage != Stage::Issued && !owner_valid(&job) {
        finish_job(base, owner, "owner body, scope or generation changed before the render");
        return;
    }
    if job.stage != Stage::Issued && !scene_writable() {
        finish_job(base, owner, "scene does not allow portrait writes");
        return;
    }
    match (job.stage, job.intent) {
        (Stage::Requested, Intent::Original { original_kind }) => {
            if job.layout.waiting() || policy::class_sub(original_kind).is_none() {
                // No construction bust to return to: the waiting card and an ordinary original go
                // back to native ownership; the product's own refresh paints them.
                release_presentation(base, job.layout, job.physical, owner, "original restored");
                finish_job(base, owner, "restore handled");
                return;
            }
            match restore_original(base, owner, &job) {
                Ok(done) => {
                    // restore_original returns Ok only with the pane read back on 0x12+owner.
                    STATE.lock().unwrap().own.unbind(owner);
                    trace!("[MiiPortrait] owner={} original restored: construction texture {}", owner, done);
                }
                Err(reason) => {
                    let mut s = STATE.lock().unwrap();
                    refuse_once(&mut s, owner, REFUSE_RESTORE, reason);
                }
            }
            finish_job(base, owner, "restore handled");
        }
        (Stage::Requested, Intent::Exact(c)) => {
            // Decide the presentation target before any helper lease: a target that cannot take
            // a bust never gets a helper installed for it. The first refusal logs its words.
            if let Err(reason) = presentation_ready(base, &job) {
                let bit = if reason == NO_PIPELINE { REFUSE_PIPELINE } else { REFUSE_PANE };
                let first = STATE.lock().unwrap().refused[owner] & bit == 0;
                if first {
                    log_presentation_words(base, &job);
                }
                {
                    let mut s = STATE.lock().unwrap();
                    refuse_once(&mut s, owner, bit, reason);
                }
                finish_job(base, owner, reason);
                return;
            }
            // Ordinary-origin scenes never request the game's portrait setup. Start only
            // its unused portrait path, and let native callbacks report completion.
            match ensure_pipeline(base, job.layout, job.kind) {
                Ok(true) => {}
                Ok(false) if job.pipeline_waits < 1200 => {
                    if let Some(current) = STATE.lock().unwrap().jobs[owner].as_mut() {
                        current.pipeline_waits += 1;
                    }
                    return;
                }
                result => {
                    let reason = result.err().unwrap_or("native portrait setup did not finish within its callback budget");
                    refuse_once(&mut STATE.lock().unwrap(), owner, REFUSE_PIPELINE, reason);
                    finish_job(base, owner, reason);
                    return;
                }
            }
            // One bust row per owner: never lease or install the helper whose row the other
            // owner's pane is bound to (`row_free_for` stays as the invariant check).
            let avoid = STATE.lock().unwrap().own.bound.iter().enumerate()
                .filter(|(o, _)| *o != owner)
                .find_map(|(_, b)| b.as_ref().map(|b| b.request.row));
            match sources::prepare_portrait(CHANNEL_BASE + owner, job.kind, c, job.holder, avoid) {
                Ok(Some(lease)) => {
                    let Some(helper_entry) = native_source::entry(lease.slot) else {
                        sources::release(lease);
                        return;
                    };
                    sources::retain_portrait(lease);
                    let mut s = STATE.lock().unwrap();
                    if let Some(current) = s.jobs[owner].as_mut() {
                        if current.generation == job.generation && current.stage == Stage::Requested {
                            current.stage = Stage::Leased;
                            current.lease = Some(lease);
                            current.helper = lease.slot;
                            current.helper_entry = helper_entry as usize;
                            trace!("[MiiPortrait] owner={} exclusive helper lease slot={} entry=0x{:x} kind=0x{:x}", owner, lease.slot, helper_entry as usize, job.kind);
                            return;
                        }
                    }
                    drop(s);
                    sources::release(lease);
                }
                Ok(None) => {}
                Err(reason) => {
                    {
                        let mut s = STATE.lock().unwrap();
                        refuse_once(&mut s, owner, REFUSE_LEASE, reason);
                    }
                    finish_job(base, owner, reason);
                }
            }
        }
        (Stage::Leased, Intent::Exact(c)) => match try_issue(base, owner, &job, c) {
            Ok((request, mirrors)) => {
                trace!("[MiiPortrait] owner={} issued render helper={} sub={} generation={} mirrors_before={:?}", owner, job.helper, request.sub, job.generation, mirrors);
            }
            Err(reason) if reason == ROW_SHARED => {
                // Terminal, not a deferral: the lease is returned at once so the helper is not
                // held against hats while the other owner's bust occupies the row.
                {
                    let mut s = STATE.lock().unwrap();
                    refuse_once(&mut s, owner, REFUSE_ROW, reason);
                }
                finish_job(base, owner, reason);
            }
            Err(reason) => {
                // The exclusive lease is held while deferring: every distinct reason is visible.
                let mut s = STATE.lock().unwrap();
                if let Some(current) = s.jobs[owner].as_mut() {
                    if current.generation == job.generation && current.stage == Stage::Leased && current.deferral != reason {
                        current.deferral = reason;
                        trace!("[MiiPortrait] owner={} render deferred helper={}: {}", owner, job.helper, reason);
                    }
                }
            }
        },
        (Stage::Leased, Intent::Original { .. }) | (Stage::Issued, _) => {}
    }
}

unsafe fn owner_valid(job: &Job) -> bool {
    let role = if job.logical == 0 { Role::Player } else { Role::Cpu };
    mii_backend::current() == Some(job.layout)
        && scope(role).is_some_and(|s| s.owner == job.physical && (job.layout != Layout::Training || s.mode == 0x04040000))
        && state_for(role).lock().unwrap().observed == Some((job.entry, job.kind))
        && mii_saved_alpha::current_generation(job.logical) == job.generation
        && active_body(job.entry as *const u8, job.physical, job.kind)
}

/// Start the native portrait-only command sequence once, before acquiring a helper.
/// Run93 observed an ordinary scene idle in command0 with both native holders null,
/// versus a Mii scene idle in command3, portrait flag1/scene flag0 and ready1.
/// The fingerprinted command1 allocates the native batch; with scene flag0 it never
/// creates `532f7a0`. Its next tick sees that null holder and skips the entire live
/// fighter builder, then native commands4/5/2 skip their scene bodies and pop.
/// Command3 sets readiness itself. We neither fabricate ready nor allocate/free
/// native objects. Training destroys its source normally; a waiting bootstrap
/// carries a retirement duty because native LDN reuses the source across a match.
/// All stores run on the service/dispatcher thread, without a native PoolLock.
unsafe fn ensure_pipeline(base: usize, layout: Layout, kind: u32) -> Result<bool, &'static str> {
    let root = base as *const u8;
    let source = pointer_chain(root, &SOURCE).ok_or("native portrait source unavailable")?;
    let batch = pointer_chain(root, &BATCH);
    let ready = read::<u8>(source, 0x328);
    let mut state = STATE.lock().unwrap();
    if let Some(expected) = state.bootstrap.as_mut() {
        if expected.source != source as usize { return Err("native portrait source changed during setup"); }
        if expected.phase != PipelinePhase::Live { return Err("owned portrait pipeline is retiring"); }
        if let Some(batch) = batch.filter(|_| ready == 1) {
            if expected.batch != 0 && expected.batch != batch as usize {
                return Err("native portrait batch changed during setup");
            }
            expected.batch = batch as usize;
            return Ok(true);
        }
        return Ok(false);
    }
    if batch.is_some() && ready == 1 { return Ok(true); }
    if !scene_writable() || pointer_chain(root, &SEQUENCE).is_none()
        || pointer_chain(root, &TABLE).is_none()
        || read::<usize>(source, 0) != source as usize
        || read::<usize>(source, 0x38) != base + 0x3302510
        || read::<usize>(source, 0x40) != 0
        || read::<usize>(source, 0x58) != base + 0x3303a80
        || read::<usize>(source, 0x60) != 0
    { return Err("native portrait setup identity or scope unavailable"); }
    let words = policy::PipelineStart {
        queue: policy::Queue { count: read(source, 0x134), word: read(source, 0x138) },
        ready, portrait_flag: read(source, 0x2d1), scene_flag: read(source, 0x2d4),
        batch_present: read::<usize>(root, BATCH_HOLDER) != 0,
        scene_object_present: read::<usize>(root, NATIVE_SCENE_HOLDER) != 0,
    };
    if !words.allowed() {
        // A native Mii scene may still be completing its own setup. Wait without
        // writing its queue or taking a helper; the caller bounds this wait.
        if words.batch_present || (1..19).contains(&words.queue.count) {
            return Ok(false);
        }
        return Err("native portrait pipeline is neither ready nor the unused ordinary pipeline");
    }
    let player_entry = native_source::entry(0).ok_or("practice P1 entry unavailable")? as usize;
    state.bootstrap = Some(Bootstrap { source: source as usize, batch: 0,
        thread: skyline::nn::os::GetCurrentThread() as usize, player_entry,
        layout, kind, phase: PipelinePhase::Live, retirement_ticks: 0, thread_deferred: false });
    let source = source as *mut u8;
    store(source, 0x2d1, 1u8);
    store(source, 0x2d4, 0u8);
    store(source, 0xe8, 3u32); // Native ready/idle after construction.
    store(source, 0x134, 1i32);
    store(source, 0xec, 1u32); // Native batch construction, not a manual allocation.
    trace!("[MiiPortrait] native portrait-only setup queued source=0x{:x}", source as usize);
    Ok(false)
}

/// Polling/UI paths only mark departure and retire plugin intents. Every native
/// write below runs on the source's captured service thread, with fresh globals.
fn request_pipeline_retirement() {
    let mut leases = [None; policy::VISIBLE_OWNERS];
    {
        let mut s = STATE.lock().unwrap();
        let Some(p) = s.bootstrap.as_mut() else { return; };
        if !p.layout.waiting() || p.phase != PipelinePhase::Live { return; }
        p.phase = PipelinePhase::Departed;
        s.own.depart();
        for (o, lease) in leases.iter_mut().enumerate() {
            if let Some(job) = s.jobs[o] {
                if job.stage == Stage::Issued {
                    s.jobs[o] = Some(Job { next: None, ..job });
                } else {
                    *lease = job.lease;
                    s.jobs[o] = None;
                }
            }
        }
        s.sync_pending();
        RETIRE_PIPELINE.store(true, Ordering::Release);
    }
    for lease in leases.iter().flatten().copied() { sources::release(lease); }
}

unsafe fn pipeline_words(base: usize, source: *const u8) -> policy::PipelineStart {
    policy::PipelineStart {
        queue: policy::Queue { count: read(source, 0x134), word: read(source, 0x138) },
        ready: read(source, 0x328), portrait_flag: read(source, 0x2d1), scene_flag: read(source, 0x2d4),
        batch_present: read::<usize>(base as *const u8, BATCH_HOLDER) != 0,
        scene_object_present: read::<usize>(base as *const u8, NATIVE_SCENE_HOLDER) != 0,
    }
}

fn retirement_failed(reason: &str) {
    // No retry loop or freeing an unqualified native object. Keep the identity
    // for diagnosis; ordinary native manager destruction still owns its memory.
    RETIRE_PIPELINE.store(false, Ordering::Release);
    ACTIVE.store(false, Ordering::Release);
    disarm();
    trace!("[MiiPortrait] owned pipeline retirement refused: {}; portraits disabled until restart", reason);
}

fn retirement_complete(reason: &str) {
    let leases = {
        let mut s = STATE.lock().unwrap();
        let leases = s.jobs.map(|j| j.and_then(|j| j.lease));
        s.jobs = [None, None];
        s.bootstrap = None;
        s.waiting_crop = None;
        s.own.teardown();
        let _ = s.own.abandon_in_flight();
        s.batch = 0;
        s.sync_pending();
        leases
    };
    RETIRE_PIPELINE.store(false, Ordering::Release);
    disarm();
    for lease in leases.iter().flatten().copied() { sources::release(lease); }
    trace!("[MiiPortrait] owned waiting pipeline retired: {}", reason);
}

/// Same-thread ownership-scoped rollback of the unused source borrowed by
/// ensure_pipeline. Start cmd0 only when the native render queue drained and the
/// waiting pane has returned to a stock icon. Never free a request ourselves.
/// Only queue the reset; the normal native tick executes it in its original lock
/// context. Never call the native dispatcher or destructor from entry teardown.
unsafe fn begin_pipeline_retirement(base: usize, source: *const u8) -> bool {
    if !RETIRE_PIPELINE.load(Ordering::Acquire) { return false; }
    let Some(p) = STATE.lock().unwrap().bootstrap else { return false; };
    if p.phase != PipelinePhase::Departed { return false; }
    if p.thread != skyline::nn::os::GetCurrentThread() as usize {
        if !p.thread_deferred {
            if let Some(current) = STATE.lock().unwrap().bootstrap.as_mut() { current.thread_deferred = true; }
            trace!("[MiiPortrait] pipeline retirement deferred to captured native service thread");
        }
        return false;
    }
    let current = pointer_chain(base as *const u8, &SOURCE);
    if current != Some(source) || p.source != source as usize {
        retirement_complete("native source replaced; no old pointer read or written");
        return false;
    }
    let batch = pointer_chain(base as *const u8, &BATCH).map_or(0, |v| v as usize);
    if p.batch != 0 && batch != p.batch {
        retirement_complete("native batch removed/replaced; no native state written");
        return false;
    }
    if read::<usize>(source, 0) != source as usize || read::<usize>(source, 0x28) != base + 0x33021b0
        || read::<usize>(source, 0x30) != 0 || read::<usize>(source, 0x58) != base + 0x3303a80
        || read::<usize>(source, 0x60) != 0 {
        retirement_failed("source callback identity changed");
        return false;
    }
    let (player_bound, cpu_bound) = {
        let s = STATE.lock().unwrap(); (s.own.bound[0].is_some(), s.own.bound[1].is_some())
    };
    if player_bound {
        retirement_failed("unexpected waiting P1 portrait binding");
        return false;
    }
    if cpu_bound {
        // Return the pane while the waiting UI is still alive, even if a departed
        // render is draining. It can no longer bind. A nulled HUD singleton is
        // authoritative absence; a failed lookup under a live root is not.
        let hud_gone = read::<usize>(base as *const u8, HUD_ROOT[0]) == 0;
        if !hud_gone && !super::restore_waiting_kind_icon(base, p.kind) {
            retirement_failed("waiting pane could not return to its kind icon before teardown");
            return false;
        }
        if hud_gone { STATE.lock().unwrap().waiting_crop = None; }
        else { restore_waiting_uv(base, 4); }
        STATE.lock().unwrap().own.unbind(1);
    }
    if STATE.lock().unwrap().own.in_flight.is_some() { drain_tick(base); }
    let words = pipeline_words(base, source);
    let flight = STATE.lock().unwrap().own.in_flight.is_some();
    if !words.retirable(read(source, 0xe8), read(source, 0x144), flight) { return false; }
    if batch == 0 { return false; }
    // No locks span the native dispatcher/destructor. An interrupted callback
    // cannot start another render once phase becomes Resetting.
    {
        let mut s = STATE.lock().unwrap();
        let Some(current) = s.bootstrap.as_mut() else { return false; };
        if current.source != p.source || current.phase != PipelinePhase::Departed { return false; }
        current.batch = batch;
        current.phase = PipelinePhase::Resetting;
    }
    disarm();
    store(source as *mut u8, 0xe8, 0u32);
    trace!("[MiiPortrait] owned waiting pipeline reset queued source=0x{:x} batch=0x{:x} thread=0x{:x} mode={:?}", p.source, batch, p.thread, crate::common::game_mode::current());
    true
}

unsafe fn finish_pipeline_retirement(base: usize, source: *const u8) {
    if !RETIRE_PIPELINE.load(Ordering::Acquire) { return; }
    let Some(p) = STATE.lock().unwrap().bootstrap else { return; };
    if p.phase != PipelinePhase::Resetting || p.thread != skyline::nn::os::GetCurrentThread() as usize { return; }
    if p.source != source as usize || pointer_chain(base as *const u8, &SOURCE) != Some(source) {
        retirement_failed("source changed during native reset");
        return;
    }
    if !pipeline_words(base, source).reset_completed(read(source, 0xe8), read(source, 0x2d0)) {
        retirement_failed("native reset readback did not match unused pipeline");
        return;
    }
    // cmd0 restores flags and frees/nulls both native holders. Its ready latch
    // is outside that reset; admission captured exactly 0. Restore it only after
    // that verified reset, never while native cmd3/new work has taken ownership.
    store(source as *mut u8, 0x328, 0u8);
    retirement_complete("native cmd0 freed holders; original ready=0 restored");
}

#[skyline::hook(offset = 0x3308f00, inline)]
unsafe fn pipeline_before_tick(ctx: &mut InlineCtx) {
    if RETIRE_PIPELINE.load(Ordering::Acquire) {
        let exhausted = {
            let mut s = STATE.lock().unwrap();
            if s.bootstrap.is_none() {
                RETIRE_PIPELINE.store(false, Ordering::Release);
                return;
            }
            s.bootstrap.as_mut().is_some_and(|p| {
                p.retirement_ticks = p.retirement_ticks.saturating_add(1);
                p.retirement_ticks > 240
            })
        };
        if exhausted {
            retirement_failed("native queue did not reach a qualified reset within 240 ticks");
            return;
        }
        begin_pipeline_retirement(BASE.load(Ordering::Acquire), ctx.registers[19].x() as *const u8);
    }
}

#[skyline::hook(offset = 0x3308f30, inline)]
unsafe fn pipeline_after_tick(ctx: &mut InlineCtx) {
    if RETIRE_PIPELINE.load(Ordering::Acquire) {
        finish_pipeline_retirement(BASE.load(Ordering::Acquire), ctx.registers[19].x() as *const u8);
    }
}

/// Admission and issue, all words fresh, nothing retained. Returns Err silently on every
/// deferral; the caller logs only the transition.
unsafe fn try_issue(base: usize, owner: usize, job: &Job, c: Configuration) -> Result<(policy::Request, Option<Mirrors>), &'static str> {
    if !scene_writable() {
        return Err("scene does not allow portrait writes");
    }
    let lease = job.lease.ok_or("no lease")?;
    let h = job.helper;
    if mii_backend::current().is_none_or(|l| !l.helpers().contains(&h)) {
        return Err("leased slot is not a helper row");
    }
    if !policy::row_free_for(&STATE.lock().unwrap().own.bound, owner, h) {
        return Err(ROW_SHARED);
    }
    if !sources::portrait_lease_valid(lease, job.holder) {
        return Err("portrait lease no longer valid");
    }
    let helper_entry = native_source::entry(h).ok_or("helper entry unavailable")?;
    if helper_entry as usize != job.helper_entry {
        return Err("helper entry identity changed");
    }
    let ready = sources::portrait_ready(lease, job.holder).ok_or("helper source not ready")?;
    let parked = native_source::parked_ready(helper_entry, h, job.kind);
    let exact = mii_managed_helper::ready(ready, c);
    let root = base as *const u8;
    let source = pointer_chain(root, &SOURCE).ok_or("native source unavailable")?;
    let sequence = pointer_chain(root, &SEQUENCE).ok_or("HUD sequence unavailable")?;
    let internal = pointer_chain(root, &FIGHTERS).ok_or("fighter table unavailable")?;
    if read::<usize>(internal, 0x20 + h as usize * 8) != job.helper_entry {
        return Err("helper row does not hold the leased entry");
    }
    let batch = pointer_chain(root, &BATCH).ok_or("render batch unavailable")?;
    let (pane_index, pane_bound) = pane_state(base, job)?;
    let admission = policy::Admission {
        sequence_pending: read(sequence, 0xd),
        sequence_busy: read(sequence, 0x1dfe8),
        source_ready: read(source, 0x328),
        count: read(source, 0x134),
        word: read(source, 0x138),
        template_gate: read(internal, 0xb20),
        scene_byte: read(root, SCENE_BYTE),
        skip_armed: ACTIVE.load(Ordering::Acquire),
        install_active: INSTALL.active(),
        helper_parked: parked,
        helper_exact: exact,
        lease_valid: true,
        pane_index,
        pane_bound,
    };
    policy::admit(admission, owner as u32, h)?;
    let words = policy::PORTRAIT_SUBS.map(|sub| request_words(request_at(batch, h, sub)));
    let mut s = STATE.lock().unwrap();
    if s.jobs[owner].is_none_or(|j| j.generation != job.generation || j.stage != Stage::Leased) {
        return Err("job changed");
    }
    if s.halted {
        return Err("portrait halted for this scene");
    }
    let request = match s.own.select(h, words) {
        policy::Select::Request { request, .. } => request,
        policy::Select::Defer(reason) | policy::Select::Refuse(reason) => return Err(reason),
    };
    let req = request_at(batch, h, request.sub).ok_or("request unavailable")?;
    let before_table = table_at(base, policy::table_index(h)).unwrap_or(0);
    let mirrors = mirrors_of(job.helper_entry);
    s.own.issue(policy::Job::new(owner, request, job.generation, before_table))?;
    if let Some(current) = s.jobs[owner].as_mut() {
        current.stage = Stage::Issued;
        current.request = Some(request);
        current.before = mirrors;
        current.source = source as usize;
        current.batch = batch as usize;
        current.hits = HOOK_HITS.load(Ordering::Acquire);
        current.skips = HOOK_SKIPS.load(Ordering::Acquire);
        current.camera = CAMERA_OVERRIDES.load(Ordering::Acquire);
        current.draw = DRAW_OVERRIDES.load(Ordering::Acquire);
    }
    arm(req as usize, job.helper_entry, h, request.sub);
    issue(base, source as *mut u8, h, request.sub);
    s.sync_pending();
    Ok((request, mirrors))
}

/// `13437b0`'s single-item stores in native order. Count is published before the command
/// word, so this is sound only on the thread that runs `13437b0` (the service thread, run38).
unsafe fn issue(base: usize, source: *mut u8, row: u32, sub: u32) {
    if !scene_writable() {
        return;
    }
    store(source, 0x2e4, sub);
    store(source, 0x2e0, row);
    let placement = read::<[u64; 2]>(base as *const u8, PLACEMENT);
    store(source, 0x2f0, placement);
    store(source, 0x300, 0u16);
    for (count, command) in [(1i32, 0xbu32), (2, 0xa)] {
        store(source, 0x134, count);
        store(source, 0xe8 + count as usize * 4, command);
    }
}

unsafe fn drain_tick(base: usize) {
    let Some(job) = STATE.lock().unwrap().own.in_flight else { return; };
    let root = base as *const u8;
    let Some(source) = pointer_chain(root, &SOURCE) else {
        // The native source object itself is gone: nothing can drain. Authoritative.
        let abandoned = { let mut s = STATE.lock().unwrap(); let a = s.own.abandon_in_flight(); s.sync_pending(); a };
        if abandoned.is_some() {
            disarm();
            trace!("[MiiPortrait] owner={} native source object gone; queued render abandoned", job.owner);
            finish_job(base, job.owner, "native source object gone");
        }
        return;
    };
    // The render belongs to the S' and batch observed at issue. Any other object is not the
    // queue or request this job published: latch departure (never bind) and keep waiting.
    let issued = STATE.lock().unwrap().jobs[job.owner].map(|j| (j.source, j.batch));
    let batch_now = pointer_chain(root, &BATCH).map_or(0, |b| b as usize);
    if issued.is_none_or(|(issued_source, _)| issued_source != source as usize) {
        // A different source's idle queue cannot complete the old source's work. This
        // unqualified lifetime disables this private feature until restart; otherwise a
        // later scene could acquire new leases behind an orphaned in-flight record.
        // Existing leases remain in the book until its native departure/destruction cleanup.
        let mut s = STATE.lock().unwrap();
        s.own.depart();
        if !s.halted {
            trace!("[MiiPortrait] owner={} source identity changed; portrait feature disabled until restart, native cleanup retains ownership", job.owner);
        }
        s.halted = true;
        disarm();
        ACTIVE.store(false, Ordering::Release);
        return;
    }
    if issued != Some((source as usize, batch_now)) && !job.departed {
        STATE.lock().unwrap().own.depart();
        disarm();
        trace!("[MiiPortrait] owner={} issue-time source/batch identity changed; render will not bind", job.owner);
    }
    let queue = policy::Queue { count: read(source, 0x134), word: read(source, 0x138) };
    let same_target = read::<u32>(source, 0x2e0) == job.request.row && read::<u32>(source, 0x2e4) == job.request.sub;
    let (words, table, descriptor) = match pointer_chain(root, &BATCH).and_then(|batch| request_at(batch, job.request.row, job.request.sub)) {
        Some(req) => (request_words(Some(req)), table_at(base, policy::table_index(job.request.row)).unwrap_or(0), req as usize + 0x180),
        None => (policy::RequestWords::default(), 0, 0),
    };
    let outcome = {
        let mut s = STATE.lock().unwrap();
        let outcome = s.own.complete(policy::Completion { queue, table, descriptor }, words, same_target);
        if !matches!(outcome, policy::Outcome::Wait) {
            disarm();
            s.sync_pending();
        }
        outcome
    };
    match outcome {
        policy::Outcome::Wait => {}
        policy::Outcome::Discard(reason) => {
            trace!("[MiiPortrait] owner={} render discarded: {} (count={} word=0x{:x})", job.owner, reason, queue.count, queue.word);
            finish_job(base, job.owner, reason);
        }
        policy::Outcome::Bind { owner, index } => {
            let adapter = STATE.lock().unwrap().jobs[owner];
            let proof = match adapter {
                Some(current) if current.stage == Stage::Issued => after_render_proof(&current),
                _ => Err("no issued adapter job"),
            };
            if let (Some(current), Err(reason)) = (adapter, proof) {
                if current.stage == Stage::Issued && current.lease.is_some() {
                    // Fail closed: no bind, and the helper is not returned to the book. Its
                    // portrait lease is held until scene departure clears every lease.
                    let mut s = STATE.lock().unwrap();
                    s.halted = true;
                    if let Some(j) = s.jobs[owner].as_mut() { j.lease = None; }
                    drop(s);
                    trace!("[MiiPortrait] owner={} after-render proof failed: {}; not bound, helper slot={} quarantined until scene departure", owner, reason, current.helper);
                    finish_job(base, owner, reason);
                    return;
                }
            }
            match adapter {
                Some(current) if current.stage == Stage::Issued && proof.is_ok() && owner_valid(&current) => {
                    let renders = HOOK_HITS.load(Ordering::Acquire).wrapping_sub(current.hits);
                    let camera = CAMERA_OVERRIDES.load(Ordering::Acquire).wrapping_sub(current.camera);
                    let draw = DRAW_OVERRIDES.load(Ordering::Acquire).wrapping_sub(current.draw);
                    if camera != renders || draw != renders {
                        trace!("[MiiPortrait] owner={} framing not applied: renders={} camera={} draw={}; native portrait retained", owner, renders, camera, draw);
                        finish_job(base, owner, "render framing unavailable");
                        return;
                    }
                    match bind_target(base, &current, index, descriptor) {
                        Ok(slot) => {
                            STATE.lock().unwrap().own.bound(owner, policy::Binding { request: job.request, descriptor });
                            trace!("[MiiPortrait] owner={} bound index=0x{:x} slot={} layout={:?} request=({},{}) descriptor=0x{:x}", current.physical, index, slot, current.layout, job.request.row, job.request.sub, descriptor);
                        }
                        Err(reason) => {
                            let mut s = STATE.lock().unwrap();
                            refuse_once(&mut s, owner, REFUSE_PANE, reason);
                        }
                    }
                }
                _ => trace!("[MiiPortrait] owner={} render completed but the owner changed; not bound, request kept free", owner),
            }
            finish_job(base, owner, "render completed");
        }
    }
}

/// End the owner's job: after-render proof once, lease release outside every lock, then
/// start a superseding intent if one was queued.
unsafe fn finish_job(base: usize, owner: usize, reason: &str) {
    let job = {
        let mut s = STATE.lock().unwrap();
        let job = s.jobs[owner].take();
        s.sync_pending();
        job
    };
    let Some(job) = job else { return; };
    if job.stage == Stage::Issued {
        // Read the helper only while this callback proves it alive: valid lease and the same
        // entry in the native slot. Departed or destroyed helpers carry helper_entry 0.
        let live = job.helper_entry != 0
            && job.lease.is_some_and(|lease| sources::portrait_lease_valid(lease, job.holder))
            && native_source::entry(job.helper).is_some_and(|e| e as usize == job.helper_entry);
        let after = if live { mirrors_of(job.helper_entry) } else { None };
        let (parked, exact) = match (job.lease, job.intent) {
            (Some(lease), Intent::Exact(c)) if sources::portrait_lease_valid(lease, job.holder) => {
                let entry = native_source::entry(job.helper);
                let parked = entry.is_some_and(|e| e as usize == job.helper_entry && native_source::parked_ready(e, job.helper, job.kind));
                let exact = sources::portrait_ready(lease, job.holder).is_some_and(|ready| mii_managed_helper::ready(ready, c));
                (Some(parked), Some(exact))
            }
            _ => (None, None),
        };
        trace!("[MiiPortrait] owner={} after render helper={} reason=\"{}\" parked={:?} exact={:?} mirrors_before={:?} mirrors_after={:?} equal={} hook_hits={} hook_skips={} camera={} draw={}",
            owner, job.helper, reason, parked, exact, job.before, after, job.before == after,
            HOOK_HITS.load(Ordering::Relaxed), HOOK_SKIPS.load(Ordering::Relaxed),
            CAMERA_OVERRIDES.load(Ordering::Relaxed).wrapping_sub(job.camera),
            DRAW_OVERRIDES.load(Ordering::Relaxed).wrapping_sub(job.draw));
    }
    if let Some(lease) = job.lease {
        // Discard/supersession also ends a native render. Returning the helper without
        // this proof would bypass the bind-path quarantine and stall its next consumer.
        let proof = if job.stage == Stage::Issued { helper_release_proof(&job) } else { Ok(()) };
        match proof {
            Ok(()) => sources::release(lease),
            Err(reason) => {
                STATE.lock().unwrap().halted = true;
                trace!("[MiiPortrait] owner={} helper release refused: {}; slot={} retained until scene departure", owner, reason, job.helper);
            }
        }
    }
    sources::cancel(CHANNEL_BASE + owner);
    let _ = base;
    if let Some(next) = job.next {
        let intent = match next.intent {
            Intent::Exact(c) => PortraitIntent::Exact(c),
            Intent::Original { original_kind } => PortraitIntent::Original { original_kind },
        };
        request(next.entry as *const u8, next.physical, next.generation, next.kind, intent);
    }
}

/// Gate for binding a drained render and returning its helper: every armed render of this job
/// took the scoped skip, and the helper is still the same live, parked, exact source with
/// unchanged D mirrors (the template copy at 32c7410.. would rewrite them).
unsafe fn after_render_proof(job: &Job) -> Result<(), &'static str> {
    if HOOK_HITS.load(Ordering::Acquire).wrapping_sub(job.hits) == 0 {
        return Err("no owned render observed for this publication");
    }
    helper_release_proof(job)
}

/// A discarded job may have been replaced before its render (zero hits). It may return
/// an untouched parked helper, but every render that did execute must have skipped the
/// template and the full helper proof must still hold. Never release on logging alone.
unsafe fn helper_release_proof(job: &Job) -> Result<(), &'static str> {
    let (Some(lease), Intent::Exact(c)) = (job.lease, job.intent) else {
        return Err("no live exact portrait lease");
    };
    let hits = HOOK_HITS.load(Ordering::Acquire).wrapping_sub(job.hits);
    let skips = HOOK_SKIPS.load(Ordering::Acquire).wrapping_sub(job.skips);
    if skips != hits {
        return Err("armed render did not take the scoped template skip");
    }
    if job.helper_entry == 0 || !sources::portrait_lease_valid(lease, job.holder) {
        return Err("portrait lease no longer valid");
    }
    let entry = native_source::entry(job.helper).ok_or("helper entry unavailable")?;
    let internal = pointer_chain(BASE.load(Ordering::Acquire) as *const u8, &FIGHTERS).ok_or("fighter table unavailable")?;
    if entry as usize != job.helper_entry || read::<usize>(internal, 0x20 + job.helper as usize * 8) != job.helper_entry {
        return Err("helper entry identity changed");
    }
    if !native_source::parked_ready(entry, job.helper, job.kind) {
        return Err("helper not parked after render");
    }
    let ready = sources::portrait_ready(lease, job.holder).ok_or("helper source not ready after render")?;
    if !mii_managed_helper::ready(ready, c) {
        return Err("helper configuration not exact after render");
    }
    if job.before.is_none() || mirrors_of(job.helper_entry) != job.before {
        return Err("helper D mirrors changed by the render");
    }
    Ok(())
}

/// Explicit original restoration: rebind the construction texture while it is still registered.
unsafe fn restore_original(base: usize, owner: usize, job: &Job) -> Result<bool, &'static str> {
    let Intent::Original { original_kind } = job.intent else { return Err("not an original intent"); };
    let sub = policy::class_sub(original_kind).ok_or("original is not a Mii class")?;
    let batch = pointer_chain(base as *const u8, &BATCH).ok_or("render batch unavailable")?;
    let _ = owner;
    let req = request_at(batch, job.physical, sub).ok_or("construction request unavailable")?;
    let index = policy::table_index(job.physical);
    let table = table_at(base, index).unwrap_or(0);
    let ok = policy::restore_original(policy::Intent::Original { original_kind }, job.kind, job.physical, request_words(Some(req)), table, req as usize + 0x180);
    if !ok {
        return Err("construction portrait is no longer registered; original left as is");
    }
    let (_, slot_ptr) = visible_slot(base, job.physical)?;
    if read::<u32>(slot_ptr, 0x1ac) == index && read::<u8>(slot_ptr, 0x1a9) == 1 {
        return Ok(false);
    }
    rebind(base, job.physical, index)?;
    Ok(true)
}

/// Decide the presentation target before any helper lease.
/// Training: the owner's HUD slot must carry the part chain `141b8a0` dereferences
/// (`part_ready`). A slot not in bust mode (`+0x1a8 == 0`) or never bound (`+0x1a9 == 0`) is
/// accepted; `bind_target` sets the mode and `141b8a0` writes `+0x1a9/+0x1ac` itself.
/// Waiting: the render source/sequence/table and the owner's card pane must resolve.
/// Batch creation is admitted separately by ensure_pipeline, before any helper lease.
unsafe fn presentation_ready(base: usize, job: &Job) -> Result<(), &'static str> {
    match job.layout {
        Layout::Training => {
            let (_, slot_ptr) = slot_of_owner(base, job.physical)?;
            if !part_ready(slot_ptr) {
                return Err(NO_PART);
            }
            Ok(())
        }
        Layout::Ldn | Layout::Quickplay => {
            if !pipeline_present(base) {
                return Err(NO_PIPELINE);
            }
            waiting_uv(base, job.physical).map(|_| ())
        }
    }
}

unsafe fn pipeline_present(base: usize) -> bool {
    let root = base as *const u8;
    pointer_chain(root, &SOURCE).is_some() && pointer_chain(root, &SEQUENCE).is_some()
        && pointer_chain(root, &TABLE).is_some()
}

/// First-refusal capture: the words that decide the scope on the next verification.
unsafe fn log_presentation_words(base: usize, job: &Job) {
    match job.layout {
        Layout::Training => {
            if let Ok((slot, slot_ptr)) = slot_of_owner(base, job.physical) {
                log_slot_words(base, job.logical as usize, slot, slot_ptr);
            }
        }
        Layout::Ldn | Layout::Quickplay => {
            let root = base as *const u8;
            let at = |chain: &[usize]| pointer_chain(root, chain).map_or(0, |p| p as usize);
            trace!("[MiiPortrait] owner={} waiting pipeline words source=0x{:x} sequence=0x{:x} batch=0x{:x} table=0x{:x} card_pane=0x{:x}",
                job.physical, at(&SOURCE), at(&SEQUENCE), at(&BATCH), at(&TABLE),
                super::waiting_icon_pane(base, job.physical).map_or(0, |p| p as usize));
        }
    }
}

/// Admission view of the target, `(pane_index, pane_bound)` for `policy::admit`. A slot not
/// yet bound in bust mode reports index 0: its `+0x1ac` is unobserved until `141b8a0` writes it.
unsafe fn pane_state(base: usize, job: &Job) -> Result<(u32, bool), &'static str> {
    match job.layout {
        Layout::Training => {
            let (_, slot_ptr) = slot_of_owner(base, job.physical)?;
            if !part_ready(slot_ptr) {
                return Err(NO_PART);
            }
            let index = if read::<u8>(slot_ptr, 0x1a8) != 0 && read::<u8>(slot_ptr, 0x1a9) == 1 {
                read::<u32>(slot_ptr, 0x1ac)
            } else {
                0
            };
            Ok((index, true))
        }
        Layout::Ldn | Layout::Quickplay => super::waiting_icon_pane(base, job.physical).map(|_| (0, true)).ok_or("waiting card pane unavailable"),
    }
}

/// Bind the registered descriptor for the owner's target.
/// Training: `141b8a0(slots, slot, index)` as at construction. A slot not in bust mode gets
/// `+0x1a8 = 1` first, the byte `141b8a0` gates on (`141b8c4`); its original
/// `+0x1a8/+0x1a9/+0x1ac` are recorded. If `141b8a0` does not take the index it returned before
/// its stores and pane bind (`141b90c/141b910` precede `141b954`), so the saved mode is written
/// back. The construction key at `+0x170` is left untouched; framing follows that key.
/// Waiting: `379e9c0([card+8], descriptor, 0)` binds the native 1024px render;
/// its existing UV quad frames the face/shoulders, with a retained native baseline.
unsafe fn bind_target(base: usize, job: &Job, index: u32, descriptor: usize) -> Result<i32, &'static str> {
    if !scene_writable() {
        return Err("scene does not allow portrait writes");
    }
    match job.layout {
        Layout::Training => {
            let (slot, slot_ptr) = slot_of_owner(base, job.physical)?;
            if table_at(base, index).unwrap_or(0) == 0 {
                return Err("table slot is empty");
            }
            if !part_ready(slot_ptr) {
                return Err(NO_PART);
            }
            let original = Flipped {
                slot,
                mode: read(slot_ptr, 0x1a8),
                bound_flag: read(slot_ptr, 0x1a9),
                index: read(slot_ptr, 0x1ac),
            };
            let first_bind = original.mode == 0 || original.bound_flag != 1;
            if original.mode == 0 {
                store(slot_ptr as *mut u8, 0x1a8, 1u8);
            }
            if let Err(reason) = rebind_slot(base, slot, slot_ptr, index) {
                store(slot_ptr as *mut u8, 0x1a8, original.mode);
                return Err(reason);
            }
            if first_bind {
                let mut s = STATE.lock().unwrap();
                let l = job.logical as usize;
                if s.flipped[l].is_none() {
                    s.flipped[l] = Some(original);
                }
                trace!("[MiiPortrait] owner={} slot {} entered bust mode (was +0x1a8={} +0x1a9={} +0x1ac=0x{:x}); key +0x170 untouched", job.physical, slot, original.mode, original.bound_flag, original.index);
            }
            Ok(slot)
        }
        Layout::Ldn | Layout::Quickplay => {
            let (pane, coordinates) = waiting_uv(base, job.physical)?;
            let current = read::<[f32; 8]>(coordinates, 0);
            let saved = STATE.lock().unwrap().waiting_crop;
            let original = saved.filter(|s| s.pane == pane as usize && s.coordinates == coordinates as usize
                && s.applied == current).map_or(current, |s| s.original);
            let applied = policy::waiting_portrait_uv(original).ok_or("waiting UV coordinates invalid")?;
            let overlay: unsafe extern "C" fn(*const u8, usize, i32) = std::mem::transmute(base + PANE_OVERRIDE);
            overlay(pane, descriptor, 0);
            set_waiting_uv(base, pane, &applied);
            STATE.lock().unwrap().waiting_crop = Some(WaitingCrop { pane: pane as usize,
                coordinates: coordinates as usize, original, applied });
            if let Some(p) = STATE.lock().unwrap().bootstrap.as_mut() { p.kind = job.kind; }
            Ok(-1)
        }
    }
}

/// `nn::ui2d::Picture`'s in-place TexCoordArray at +0xf0. Run95 and native
/// `5e640..5e688` establish capacity/count, quad pointer and setter ABI. The
/// fresh generation-checked card and exact Picture vtable qualify this borrow.
/// No UV allocation, resize, material replacement or reference-count operation.
unsafe fn waiting_uv(base: usize, physical: u32) -> Result<(*const u8, *const u8), &'static str> {
    let card = super::waiting_icon_pane(base, physical).ok_or("waiting card unavailable")?;
    let pane = pointer_chain(card, &[8]).ok_or("waiting inner pane unavailable")?;
    if read::<usize>(pane, 0) != base + 0x5240748 {
        return Err("waiting Picture type changed");
    }
    let capacity = read::<u8>(pane, 0xf0);
    let count = read::<u8>(pane, 0xf1);
    if capacity > 3 || count == 0 || count > capacity {
        return Err("waiting Picture has no existing UV quad");
    }
    let coordinates = pointer_chain(pane, &[0xf8]).ok_or("waiting UV storage unavailable")?;
    Ok((pane, coordinates))
}

/// Caller has just qualified `waiting_uv` in the same engine service callback.
/// `445d0(pane+f0, 0, quad)` copies eight floats; it does not retain the source.
unsafe fn set_waiting_uv(base: usize, pane: *const u8, quad: &[f32; 8]) {
    let set: unsafe extern "C" fn(*const u8, i32, *const f32) = std::mem::transmute(base + 0x445d0);
    set(pane.add(0xf0), 0, quad.as_ptr());
}

unsafe fn restore_waiting_uv(base: usize, physical: u32) {
    let Some(saved) = STATE.lock().unwrap().waiting_crop.take() else { return; };
    let Ok((pane, coordinates)) = waiting_uv(base, physical) else { return; };
    // Do not overwrite a different pane, a resized quad, or a later native/other
    // owner's edit. Stored addresses are never dereferenced to perform the undo.
    if saved.pane == pane as usize && saved.coordinates == coordinates as usize
        && read::<[f32; 8]>(coordinates, 0) == saved.applied {
        set_waiting_uv(base, pane, &saved.original);
    }
}

/// `141b8a0`'s unconditional work after its table gate: part index `slot+0xe8` in 0..8, else
/// `141b914 b.hs 141ba98` aborts; `[slot+0xd0]`, `[[slot+0xd0]+0x80]` and
/// `[parts + idx*0xd90 + 0x330]` are dereferenced (`141b920..141b934`); and `x22 = [slot+0xe0]`
/// (`141b96c`) is dereferenced as `[x22]` and passed to `13fc300` (`141b9f4..141b9f8`), whose
/// result is read at `141b9fc`. A construction-bound slot already passed these; a slot this
/// module binds first must be checked before the call.
unsafe fn part_ready(slot_ptr: *const u8) -> bool {
    // Validate before computing or reading the indexed native child.
    let Some(offset) = policy::part_offset(read::<i32>(slot_ptr, 0xe8)) else {
        return false;
    };
    pointer_chain(slot_ptr, &[0xd0, 0x80])
        .is_some_and(|parts| pointer_chain(parts, &[offset]).is_some())
        && pointer_chain(slot_ptr, &[0xe0, 0]).is_some()
}

unsafe fn rebind_slot(base: usize, slot: i32, slot_ptr: *const u8, index: u32) -> Result<(), &'static str> {
    if !scene_writable() {
        return Err("scene does not allow portrait writes");
    }
    if !part_ready(slot_ptr) { return Err(NO_PART); }
    let hud = pointer_chain(base as *const u8, &HUD_ROOT).ok_or("HUD root unavailable")?;
    let slots = pointer_chain(hud, &[0xb0, 0x1790]).ok_or("HUD slots unavailable")?;
    let rebind: unsafe extern "C" fn(*const u8, i32, i32) = std::mem::transmute(base + REBIND);
    rebind(slots, slot, index as i32);
    if read::<u32>(slot_ptr, 0x1ac) != index || read::<u8>(slot_ptr, 0x1a9) != 1 {
        return Err("pane did not take the new index");
    }
    Ok(())
}

/// Return the owner's presentation to native ownership. Training: nothing is written. The slot
/// is in Mii mode (by construction or by `bind_target`), so the event `0x1d` that follows writes
/// the non-Mii key group into the slot (`1363bd4..1363bfc`) exactly as for a Mii-origin slot,
/// the native path observed to present the normal fighter (hud48 request11-bowser).
/// Waiting: the product's kind-keyed `3778b80` assignment that follows retires the override
/// (`379e590..379e5cc`). The bound record is kept in both layouts (see below).
unsafe fn release_presentation(base: usize, layout: Layout, physical: u32, logical: usize, reason: &str) {
    // Ordinary publication keeps +0x1a8. Event 0x1d paints the new key. The bound record
    // stays until a verified bind, a verified original rebind, or batch teardown.
    let (flipped, was_bound) = {
        let s = STATE.lock().unwrap();
        (s.flipped[logical], s.own.bound[logical].is_some())
    };
    let _ = base;
    match (layout, flipped) {
        (Layout::Training, Some(f)) => {
            trace!("[MiiPortrait] owner={} slot {} stays in Mii mode (was +0x1a8={} +0x1a9={} +0x1ac=0x{:x}); native event 0x1d presents the non-Mii key; {}", physical, f.slot, f.mode, f.bound_flag, f.index, reason);
        }
        (Layout::Ldn | Layout::Quickplay, _) if was_bound => {
            trace!("[MiiPortrait] owner={} waiting card returned to the kind icon; {}", physical, reason);
        }
        _ => {}
    }
}

/// From `publish_mii_presentation` and generic roster completion, right before native
/// refresh. Waiting cards restore their UV baseline before every kind assignment.
/// A non-Mii publication also returns presentation to the native static/kind icon.
pub(super) unsafe fn before_refresh(entry: *const u8, owner: u32, kind: u32) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let base = BASE.load(Ordering::Acquire);
    let Some(layout) = mii_backend::current() else { return; };
    let Some(logical) = layout.logical(owner) else { return; };
    if base == 0 {
        return;
    }
    if layout.waiting() && owner == 4 {
        restore_waiting_uv(base, owner);
    }
    if (0x48..=0x4a).contains(&kind) { return; }
    let _ = entry;
    release_presentation(base, layout, owner, logical, "non-Mii kind published");
}

/// The visible owner's HUD slot through its record (`+0xd68` row, `+0xd60` slot), never a scan
/// of pane indices. No bust-mode judgement here.
unsafe fn slot_of_owner(base: usize, owner: u32) -> Result<(i32, *const u8), &'static str> {
    let hud = pointer_chain(base as *const u8, &HUD_ROOT).ok_or("HUD root unavailable")?;
    let slots = pointer_chain(hud, &[0xb0, 0x1790]).ok_or("HUD slots unavailable")?;
    let slot = (0..8)
        .map(|k| hud.add(k * 0x1d8))
        .find(|record| read::<i32>(*record, 0xd68) == owner as i32)
        .map(|record| read::<i32>(record, 0xd60))
        .ok_or("no HUD record for this owner")?;
    if !(0..8).contains(&slot) {
        return Err("HUD record slot out of range");
    }
    Ok((slot, slots.add(slot as usize * 0x200)))
}

/// Mii bust mode as the native readers see it: `141b8a0` (`141b8c4`), event `0x1d` (`1363bd4`),
/// the reset path (`1510e90`) and the applier updaters (`1418860`, `1418ce0`) all gate on
/// `slot+0x1a8 != 0`; `+0x1a9 == 1` is the construction bind (`13bb2f0 -> 141b8a0`).
unsafe fn bust_mode(slot_ptr: *const u8) -> bool {
    read::<u8>(slot_ptr, 0x1a8) != 0 && read::<u8>(slot_ptr, 0x1a9) == 1
}

/// A bust-mode slot, or the unsupported-scope refusal.
unsafe fn visible_slot(base: usize, owner: u32) -> Result<(i32, *const u8), &'static str> {
    let (slot, slot_ptr) = slot_of_owner(base, owner)?;
    if !bust_mode(slot_ptr) {
        return Err(NO_BUST);
    }
    Ok((slot, slot_ptr))
}

/// One-time capture for the ordinary-origin scope: the slot words whose construction values
/// are unobserved (`+0x1a8` bust mode, `+0x1ac` bound index, `+0x170` construction key,
/// `+0x120/+0x140` event key and dirty flag) and the table entry a bind would use.
unsafe fn log_slot_words(base: usize, owner: usize, slot: i32, slot_ptr: *const u8) {
    let index = policy::table_index(owner as u32);
    trace!("[MiiPortrait] owner={} slot={} words +0x1a8={} +0x1a9={} +0x1ac=0x{:x} +0x170=0x{:x} +0x120=0x{:x} +0x140={} table[0x{:x}]=0x{:x}",
        owner, slot,
        read::<u8>(slot_ptr, 0x1a8), read::<u8>(slot_ptr, 0x1a9), read::<u32>(slot_ptr, 0x1ac),
        read::<u64>(slot_ptr, 0x170), read::<u64>(slot_ptr, 0x120), read::<u8>(slot_ptr, 0x140),
        index, table_at(base, index).unwrap_or(0));
}

unsafe fn rebind(base: usize, owner: u32, index: u32) -> Result<i32, &'static str> {
    let (slot, slot_ptr) = visible_slot(base, owner)?;
    if table_at(base, index).unwrap_or(0) == 0 {
        return Err("table slot is empty");
    }
    rebind_slot(base, slot, slot_ptr, index)?;
    Ok(slot)
}

unsafe fn request_at(batch: *const u8, row: u32, sub: u32) -> Option<*const u8> {
    let req = read::<*const u8>(batch, 0xd8 + (row as usize * 10 + sub as usize) * 8);
    aligned(req).then_some(req)
}

unsafe fn request_words(req: Option<*const u8>) -> policy::RequestWords {
    match req {
        Some(req) => policy::RequestWords { index: read(req, 0x198), registered: read(req, 0x19c), holder: read(req, 0x1a8) },
        None => policy::RequestWords { index: i32::MIN, registered: u8::MAX, holder: usize::MAX },
    }
}

unsafe fn table_at(base: usize, index: u32) -> Option<usize> {
    if index > 0x1f {
        return None;
    }
    let table = pointer_chain(base as *const u8, &TABLE)?;
    let _lock = PoolLock::acquire(base, table as usize + 0x100);
    let current = read::<usize>(table, index as usize * 8);
    Some(current)
}

unsafe fn mirrors_of(entry: usize) -> Option<Mirrors> {
    if entry == 0 {
        return None;
    }
    let d = read::<*const u8>(entry as *const u8, 0xf8);
    aligned(d).then(|| Mirrors {
        index: read(d, 0x72), outfit: read(d, 0x74), hat: read(d, 0x76), colour: read(d, 0x78),
        rebuild: read(d, 0x94), specials: read(d, 0x390),
    })
}

/// The same live request/entry proof used for template protection, without assuming that
/// callee-saved registers still contain the helper (w23 becomes the kind at 32c5ff8).
/// All addresses are rederived before reads; the stored request identity is never changed.
/// No allocation, blocking lock, native call, field mutation or per-frame polling.
unsafe fn owned_render_preset(request: usize, observed_sub: u32, helper_hint: Option<usize>) -> Option<u32> {
    let armed = ARMED_REQUEST.load(Ordering::Acquire);
    if armed == 0 || request != armed || !scene_writable() { return None; }
    let packed = ARMED_ROW_SUB.load(Ordering::Acquire);
    let key = policy::Request { row: (packed >> 32) as u32, sub: packed as u32 };
    if key.row >= 8 || !policy::PORTRAIT_SUBS.contains(&key.sub) || observed_sub != key.sub { return None; }
    let base = BASE.load(Ordering::Acquire);
    if base == 0 { return None; }
    let root = base as *const u8;
    let batch = pointer_chain(root, &BATCH)?;
    let fresh = request_at(batch, key.row, key.sub)?;
    if fresh as usize != request || read::<u32>(fresh, 0) != key.row || read::<u32>(fresh, 4) != key.sub { return None; }
    let internal = pointer_chain(root, &FIGHTERS)?;
    let entry = read::<*const u8>(internal, 0x20 + key.row as usize * 8);
    if !aligned(entry) || entry as usize != ARMED_ENTRY.load(Ordering::Acquire)
        || helper_hint.is_some_and(|hint| hint != entry as usize)
        || read::<u32>(entry, 0xe0) != key.row { return None; }
    let s = STATE.try_lock().ok()?;
    let flight = s.own.in_flight?;
    let job = s.jobs.get(flight.owner).copied().flatten()?;
    if job.stage != Stage::Issued || job.helper_entry != entry as usize
        || job.batch != batch as usize || job.lease.is_none()
        || read::<u32>(entry, 0x18) != job.kind { return None; }
    policy::render_preset(key, observed_sub, job.kind, Some(flight))
}

/// 32c5cc0 loads request.sub into x24. Only its register copy selects the native
/// camera/lighting parameter record (stride0x3c at 32c5f94 and 32c66ec). Native
/// construction chooses Brawler4/Gunner5/Sword6 at 13438e4; subs7/8/9 remain storage.
#[skyline::hook(offset = 0x32c5cc4, inline)]
unsafe fn portrait_camera(ctx: &mut InlineCtx) {
    if let Some(preset) = owned_render_preset(ctx.registers[20].x() as usize,
        ctx.registers[24].x() as u32, Some(ctx.registers[23].x() as usize)) {
        ctx.registers[24].set_x(u64::from(preset));
        CAMERA_OVERRIDES.fetch_add(1, Ordering::Relaxed);
    }
}

/// 32c69fc reloads request.sub into w10. Its [4,6] test becomes the native portrait
/// draw flag passed as w4 at 32c6e28. Keep this consistent with the camera preset;
/// otherwise the same correctly framed model still takes a different draw path.
#[skyline::hook(offset = 0x32c6a00, inline)]
unsafe fn portrait_draw(ctx: &mut InlineCtx) {
    if let Some(preset) = owned_render_preset(ctx.registers[20].x() as usize,
        ctx.registers[10].x() as u32, None) {
        ctx.registers[10].set_x(u64::from(preset));
        DRAW_OVERRIDES.fetch_add(1, Ordering::Relaxed);
    }
}

/// main+0x32c7404: `cbz w9, 32c7478` right after `ldrb w9,[x21,#0xb20]`. For the one render
/// of the armed helper request, w9 := 0 so the relocated branch takes the native flag-zero
/// path and the five template copies into the helper's D do not run. Every other render,
/// including native construction, passes untouched. Fast path: one atomic load.
#[skyline::hook(offset = 0x32c7404, inline)]
unsafe fn template_skip(ctx: &mut InlineCtx) {
    let armed = ARMED_REQUEST.load(Ordering::Acquire);
    let x20 = ctx.registers[20].x() as usize;
    if armed == 0 || x20 != armed {
        return;
    }
    if !scene_writable() {
        return;
    }
    HOOK_HITS.fetch_add(1, Ordering::Relaxed);
    let base = BASE.load(Ordering::Acquire);
    let root = base as *const u8;
    let packed = ARMED_ROW_SUB.load(Ordering::Acquire);
    let (row, sub) = ((packed >> 32) as u32, packed as u32);
    let x19 = ctx.registers[19].x() as usize;
    let x21 = ctx.registers[21].x() as usize;
    let Some(fresh_internal) = pointer_chain(root, &FIGHTERS) else { return; };
    let Some(fresh_request) = pointer_chain(root, &BATCH).and_then(|batch| request_at(batch, row, sub)) else { return; };
    if !aligned(x19 as *const u8) || !aligned(x20 as *const u8) {
        return;
    }
    let view = policy::HookView {
        x19, x20, x21,
        armed_request: armed,
        armed_entry: ARMED_ENTRY.load(Ordering::Acquire),
        req_row: read(x20 as *const u8, 0),
        req_sub: read(x20 as *const u8, 4),
        entry_owner: read(x19 as *const u8, 0xe0),
        fresh_internal: fresh_internal as usize,
        fresh_entry: read(fresh_internal, 0x20 + row as usize * 8),
        fresh_request: fresh_request as usize,
    };
    // Same thread as the service callback (run42: thread 80); try_lock only fails closed.
    let Ok(state) = STATE.try_lock() else { return; };
    if policy::skip_template(view, state.own.in_flight) {
        ctx.registers[9].set_x(0);
        HOOK_SKIPS.fetch_add(1, Ordering::Relaxed);
    }
}
