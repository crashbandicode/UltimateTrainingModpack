//! Pure policy for the helper-rendered Mii HUD portrait. Numbers in, decisions out.
//! The adapter (`mii_hud_portrait.rs`) reads every word fresh in the thread-80 service
//! callback, consults this module and acts at once. No native reads or calls live here.
//! Design: artifacts/enhancements/mii-hud54-design/DESIGN.md (A1/A2) and ROOT-FLAG-RESPONSE.md.
// The policy surface is shared with its tests; the adapter uses a subset of it.
#![allow(dead_code)]

/// Candidate request subs on a helper row. Nothing here claims native code never uses
/// them: first use requires `RequestWords::idle_unregistered`, and an owned request that
/// stops reading live is disowned.
pub const PORTRAIT_SUBS: [u32; 3] = [7, 8, 9];
pub const VISIBLE_OWNERS: usize = 2;
/// Two helper rows times three subs.
pub const OWNED_CAPACITY: usize = 6;

/// A square face/shoulder window in the native 1024px Mii render. The small waiting
/// icon has no texture-SRT storage; crop its existing quad rather than allocating
/// a new material. Bilinear interpolation preserves a pane's native UV orientation.
pub fn waiting_portrait_uv(original: [f32; 8]) -> Option<[f32; 8]> {
    if original.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return None;
    }
    let mut result = [0.0; 8];
    for (i, &(u, v)) in [(0.32, 0.22), (0.68, 0.22), (0.32, 0.58), (0.68, 0.58)].iter().enumerate() {
        for axis in 0..2 {
            let top = original[axis] * (1.0 - u) + original[2 + axis] * u;
            let bottom = original[4 + axis] * (1.0 - u) + original[6 + axis] * u;
            result[i * 2 + axis] = top * (1.0 - v) + bottom * v;
        }
    }
    Some(result)
}

#[cfg(test)]
#[test]
fn waiting_crop_preserves_orientation_and_rejects_invalid_native_coordinates() {
    let ordinary = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let flipped = [1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0];
    let a = waiting_portrait_uv(ordinary).unwrap();
    let b = waiting_portrait_uv(flipped).unwrap();
    for i in 0..4 {
        assert!((a[2*i] + b[2*i] - 1.0).abs() < 1e-6);
        assert_eq!(a[2*i+1], b[2*i+1]);
    }
    assert!((a[2] - a[0] - (a[5] - a[1])).abs() < 1e-6);
    assert!(waiting_portrait_uv([f32::NAN;8]).is_none());
    assert!(waiting_portrait_uv([f32::INFINITY;8]).is_none());
    assert!(waiting_portrait_uv([-1.0;8]).is_none());
}

/// Run93's unused ordinary-origin renderer. Only this idle state may start the
/// native portrait-only setup; an existing or busy renderer remains native-owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineStart {
    pub queue: Queue,
    pub ready: u8,
    pub portrait_flag: u8,
    pub scene_flag: u8,
    pub batch_present: bool,
    pub scene_object_present: bool,
}

impl PipelineStart {
    pub fn allowed(self) -> bool {
        self.queue == (Queue { count: 0, word: 0 })
            && self.ready == 0 && self.portrait_flag == 0 && self.scene_flag == 1
            && !self.batch_present && !self.scene_object_present
    }

    /// Only an owned, drained portrait-only pipeline may return to command 0.
    /// The adapter separately checks the source/batch/entry/thread identities and
    /// releases its visible pane before the native batch destructor can run.
    pub fn retirable(self, stack_base: u32, restart: u8, in_flight: bool) -> bool {
        self.queue == (Queue { count: 0, word: 3 }) && stack_base == 3
            && restart == 0 && !in_flight && self.ready == 1
            && self.portrait_flag == 1 && self.scene_flag == 0
            && self.batch_present && !self.scene_object_present
    }

    /// cmd0 frees native holders and resets flags but leaves ready latched at 1.
    /// Restore the originally borrowed ready=0 only after this readback, never
    /// on an existing/native-owned source or on a newly entered command3.
    pub fn reset_completed(self, stack_base: u32, flags: [u8; 5]) -> bool {
        self.queue == (Queue { count: 0, word: 0 }) && stack_base == 0
            && self.ready == 1 && self.portrait_flag == 0 && self.scene_flag == 1
            && !self.batch_present && !self.scene_object_present
            && flags == [1, 0, 0, 1, 1]
    }
}

#[cfg(test)]
#[test]
fn owned_pipeline_retirement_requires_idle_and_verified_native_reset() {
    let owned = PipelineStart { queue: Queue { count: 0, word: 3 }, ready: 1,
        portrait_flag: 1, scene_flag: 0, batch_present: true, scene_object_present: false };
    assert!(owned.retirable(3, 0, false));
    assert!(!owned.retirable(3, 0, true));
    assert!(!owned.retirable(3, 1, false));
    assert!(!owned.retirable(0, 0, false));
    for queue in [Queue { count: 1, word: 3 }, Queue { count: 0, word: 0xa }, Queue { count: 0, word: 0xb }] {
        assert!(!PipelineStart { queue, ..owned }.retirable(3, 0, false));
    }
    assert!(!PipelineStart { scene_object_present: true, ..owned }.retirable(3, 0, false));
    let retired = PipelineStart { queue: Queue { count: 0, word: 0 }, ready: 1,
        portrait_flag: 0, scene_flag: 1, batch_present: false, scene_object_present: false };
    assert!(retired.reset_completed(0, [1,0,0,1,1]));
    assert!(!owned.reset_completed(0, [1,0,0,1,1]));
    assert!(!PipelineStart { batch_present: true, ..retired }.reset_completed(0, [1,0,0,1,1]));
    assert!(!PipelineStart { scene_object_present: true, ..retired }.reset_completed(0, [1,0,0,1,1]));
    assert!(!PipelineStart { queue: Queue { count: 0, word: 3 }, ..retired }.reset_completed(0, [1,0,0,1,1]));
    assert!(!retired.reset_completed(0, [1,1,0,1,1]));
}

#[cfg(test)]
#[test]
fn unused_pipeline_admission_preserves_busy_and_existing_renderers() {
    let ordinary = PipelineStart { queue: Queue { count: 0, word: 0 }, ready: 0,
        portrait_flag: 0, scene_flag: 1, batch_present: false, scene_object_present: false };
    assert!(ordinary.allowed());
    assert!(!PipelineStart { queue: Queue { count: 1, word: 1 }, ..ordinary }.allowed());
    assert!(!PipelineStart { queue: Queue { count: 0, word: 3 }, ready: 1,
        portrait_flag: 1, scene_flag: 0, batch_present: true, ..ordinary }.allowed());
    assert!(!PipelineStart { scene_object_present: true, ..ordinary }.allowed());
    assert!(!PipelineStart { batch_present: true, ..ordinary }.allowed());
    assert!(!PipelineStart { portrait_flag: 1, ..ordinary }.allowed());
}

/// The retarget (`3304a74`) registers every request of a row at `table[row+0x12]`: the table
/// entry is per row, not per request. The native applier re-binds a pane from that entry.
pub fn table_index(row: u32) -> u32 { row + 0x12 }

/// One bust row per owner. Both visible owners' renders on the same helper row would register
/// into the same `table[0x12+row]`, and the native applier (`1418780/1418c80 -> 1b4ee40`)
/// re-binds a pane from `table[slot+0x1ac]` whenever it re-applies, so the second owner's
/// registration would replace the first owner's bust on the next re-apply. A row is free for
/// `owner` while no other owner's pane is bound to a request of that row.
pub fn row_free_for(bound: &[Option<Binding>; VISIBLE_OWNERS], owner: usize, row: u32) -> bool {
    bound.iter().enumerate().all(|(o, b)| o == owner || b.is_none_or(|b| b.request.row != row))
}

/// Native construction/render preset (`13438e4`). Storage subs7/8/9 are distinct from
/// this camera/draw preset; they must stay stable while their textures are bound.
pub fn class_sub(kind: u32) -> Option<u32> {
    match kind { 0x48 => Some(4), 0x4a => Some(5), 0x49 => Some(6), _ => None }
}

/// The renderer reads request.sub as both a storage identity and a presentation preset.
/// Only the owned render's register copies may use the class preset. Never rewrite the
/// stored request key: completion, template protection and texture lifetime use that key.
pub fn render_preset(request: Request, observed_sub: u32, kind: u32,
                     in_flight: Option<Job>) -> Option<u32> {
    let job = in_flight?;
    if job.contaminated || job.request != request || request.row >= 8
        || observed_sub != request.sub || !PORTRAIT_SUBS.contains(&request.sub) {
        return None;
    }
    class_sub(kind)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request { pub row: u32, pub sub: u32 }

/// `req+0x198`, `req+0x19c`, `req+0x1a8` read in the current callback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestWords { pub index: i32, pub registered: u8, pub holder: usize }

impl RequestWords {
    /// Allocation state (`3302f7c/3302f80/3302f88`) or released by `32c4d90`. `+0xc/+0xe`
    /// are deliberately not consulted: their allocation values were not established.
    pub fn idle_unregistered(self) -> bool { self.index == -1 && self.registered == 0 && self.holder == 0 }
    /// What one of our registered requests must still read on this row.
    pub fn owned_live(self, row: u32) -> bool {
        self.index == table_index(row) as i32 && self.registered == 1 && self.holder != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding { pub request: Request, pub descriptor: usize }

/// One issued render. `superseded`, `contaminated` and `departed` are latches: the job
/// stays in flight (and its exclusive helper lease stays held) until the native queue
/// drains, whatever happened meanwhile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub owner: usize, pub request: Request, pub generation: u64, pub before_table: usize,
    pub superseded: bool, pub contaminated: bool, pub departed: bool,
    /// `+0x19c == 0` was read after issue: the `0xa` retarget (`3304b48`) ran for this render.
    pub saw_clear: bool,
}

impl Job {
    pub const fn new(owner: usize, request: Request, generation: u64, before_table: usize) -> Self {
        Self { owner, request, generation, before_table, superseded: false, contaminated: false, departed: false, saw_clear: false }
    }
}

/// What `publish_mii_presentation` passes. Absence of a configuration is never an intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent { Exact, Original { original_kind: u32 } }

#[derive(Clone, Copy, Debug, Default)]
pub struct Ownership {
    /// Requests this module registered in the current batch, as a (row, sub) set.
    pub owned: [Option<Request>; OWNED_CAPACITY],
    /// The request each visible pane currently draws. Cleared only by a successful rebind
    /// (`bound`) or by verified batch teardown (`teardown`), never by intent cancellation.
    pub bound: [Option<Binding>; VISIBLE_OWNERS],
    pub in_flight: Option<Job>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select { Request { request: Request, first_use: bool }, Defer(&'static str), Refuse(&'static str) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { Wait, Bind { owner: usize, index: u32 }, Discard(&'static str) }

/// `S'+0x134` and `S'+0x138`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Queue { pub count: i32, pub word: u32 }

impl Queue {
    /// Both native items ran: `0xa` retargeted and rendered, `0xb` created the holder.
    pub fn drained(self) -> bool { self.count == 0 && (self.word | 1) != 0xb }
}

/// Queue plus `table[0x12+row]` under its mutex and the request's `+0x180` address.
#[derive(Clone, Copy, Debug)]
pub struct Completion { pub queue: Queue, pub table: usize, pub descriptor: usize }

impl Ownership {
    pub const fn new() -> Self {
        Self { owned: [None; OWNED_CAPACITY], bound: [None; VISIBLE_OWNERS], in_flight: None }
    }

    pub fn is_owned(&self, request: Request) -> bool { self.owned.iter().flatten().any(|r| *r == request) }
    pub fn owned_count(&self) -> usize { self.owned.iter().flatten().count() }

    fn own(&mut self, request: Request) -> bool {
        if self.is_owned(request) { return true; }
        match self.owned.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => { *slot = Some(request); true }
            None => false,
        }
    }

    fn disown(&mut self, request: Request) {
        for slot in self.owned.iter_mut() { if *slot == Some(request) { *slot = None; } }
    }

    pub fn bound_by_any(&self, request: Request) -> bool {
        self.bound.iter().flatten().any(|b| b.request == request)
    }

    /// Pick the request to retarget for `row`; `words[i]` are fresh reads of `PORTRAIT_SUBS[i]`.
    /// Never a request bound by any pane, never while a job is in flight, never a request
    /// native code appears to use.
    pub fn select(&mut self, row: u32, words: [RequestWords; 3]) -> Select {
        if self.in_flight.is_some() { return Select::Refuse("a portrait render is already in flight"); }
        let mut fresh = None;
        for (i, sub) in PORTRAIT_SUBS.iter().copied().enumerate() {
            let request = Request { row, sub };
            if self.bound_by_any(request) { continue; }
            if self.is_owned(request) {
                if words[i].owned_live(row) { return Select::Request { request, first_use: false }; }
                self.disown(request);
            }
            if fresh.is_none() && words[i].idle_unregistered() { fresh = Some(request); }
        }
        match fresh {
            Some(request) => Select::Request { request, first_use: true },
            None => Select::Defer("no free, idle request on the helper row"),
        }
    }

    /// Record the issued job; exactly one at a time across both owners.
    pub fn issue(&mut self, job: Job) -> Result<(), &'static str> {
        if self.in_flight.is_some() { return Err("publisher busy"); }
        if self.bound_by_any(job.request) { return Err("would retarget a bound request"); }
        self.in_flight = Some(job);
        Ok(())
    }

    /// A newer intent for the same owner while its job is in flight. The job keeps waiting.
    pub fn supersede(&mut self, owner: usize, generation: u64) {
        if let Some(job) = self.in_flight.as_mut() {
            if job.owner == owner && job.generation != generation { job.superseded = true; }
        }
    }

    /// Intent cancellation (owner entry destroyed, request withdrawn). The in-flight job is
    /// superseded and keeps waiting; the owner's bound record is untouched because the native
    /// pane may still draw that descriptor.
    pub fn cancel_owner(&mut self, owner: usize) {
        if let Some(job) = self.in_flight.as_mut() { if job.owner == owner { job.superseded = true; } }
    }

    /// Shared `S'+0x2e0/+0x2e4` changed before the queue drained: latch; keep waiting.
    pub fn contaminate(&mut self) {
        if let Some(job) = self.in_flight.as_mut() { job.contaminated = true; }
    }

    /// Scene departure: the in-flight job may never bind, but it is not abandoned here.
    pub fn depart(&mut self) {
        if let Some(job) = self.in_flight.as_mut() { job.departed = true; }
    }

    /// Verified batch teardown (holder `[base+0x532f758]` nulled or replaced): every request
    /// and pane binding is gone. The in-flight record is kept, marked departed, until the
    /// native queue is seen drained or the source object is gone (`abandon_in_flight`).
    pub fn teardown(&mut self) {
        self.owned = [None; OWNED_CAPACITY];
        self.bound = [None; VISIBLE_OWNERS];
        self.depart();
    }

    /// Only after authoritative proof that nothing can drain (native source object gone).
    pub fn abandon_in_flight(&mut self) -> Option<Job> { self.in_flight.take() }

    /// Evaluate the in-flight job. `same_target` is `S'+0x2e0==row && S'+0x2e4==sub` this tick.
    /// Nothing ends before the queue drains; a contaminated, superseded or departed job is
    /// then discarded, never bound, even if the fields match again.
    pub fn complete(&mut self, c: Completion, words: RequestWords, same_target: bool) -> Outcome {
        let mut job = match self.in_flight { Some(job) => job, None => return Outcome::Wait };
        if !same_target && !job.contaminated { job.contaminated = true; self.in_flight = Some(job); }
        if words.registered == 0 && !job.saw_clear { job.saw_clear = true; self.in_flight = Some(job); }
        if !c.queue.drained() { return Outcome::Wait; }
        self.in_flight = None;
        if job.contaminated {
            self.disown(job.request);
            return Outcome::Discard("contaminated: shared row/sub fields changed before the queue drained");
        }
        // Reusing the request that already occupies `table[0x12+row]` re-registers the same
        // descriptor, so an unchanged table pointer is fresh only if the retarget was seen.
        let registered = words.owned_live(job.request.row)
            && c.descriptor != 0 && c.table == c.descriptor
            && (job.saw_clear || c.table != job.before_table);
        if !registered {
            self.disown(job.request);
            return Outcome::Discard("queue drained without a fresh registration of our request");
        }
        if !self.own(job.request) { return Outcome::Discard("ownership set full; request left unbound"); }
        if job.departed { return Outcome::Discard("scene departed before completion; request kept free"); }
        if job.superseded { return Outcome::Discard("superseded before completion; request kept free"); }
        Outcome::Bind { owner: job.owner, index: table_index(job.request.row) }
    }

    /// After `141b8a0` succeeded and `slot+0x1ac` was verified. The owner's previous request
    /// becomes free (still owned); that is the only non-teardown way a bound record ends.
    pub fn bound(&mut self, owner: usize, binding: Binding) {
        if owner < VISIBLE_OWNERS { self.bound[owner] = Some(binding); }
    }

    /// After a verified explicit original rebind (pane read back on `0x12+owner`): the custom
    /// request is no longer drawn. It stays owned with its texture untouched; only a later
    /// `select` may retarget it. Never called on intent alone.
    pub fn unbind(&mut self, owner: usize) {
        if owner < VISIBLE_OWNERS { self.bound[owner] = None; }
    }

    /// Per-tick audit of owned requests on `row`; returns owners whose bound request was lost.
    pub fn audit(&mut self, row: u32, words: [RequestWords; 3]) -> [bool; VISIBLE_OWNERS] {
        let mut lost = [false; VISIBLE_OWNERS];
        for (i, sub) in PORTRAIT_SUBS.iter().copied().enumerate() {
            let request = Request { row, sub };
            if !self.is_owned(request) || words[i].owned_live(row) { continue; }
            self.disown(request);
            for (owner, b) in self.bound.iter_mut().enumerate() {
                if b.is_some_and(|b| b.request == request) { *b = None; lost[owner] = true; }
            }
        }
        lost
    }
}

/// Words read before issuing the two native commands.
#[derive(Clone, Copy, Debug)]
pub struct Admission {
    pub sequence_pending: u8, pub sequence_busy: u8, pub source_ready: u8, // G+0xd, G+0x1dfe8, S'+0x328
    pub count: i32, pub word: u32,                                        // S'+0x134, S'+0x138
    pub template_gate: u8, pub scene_byte: u8,                            // internal+0xb20, base+0x52c51b0
    /// The 32c7404 scoped skip is installed (fingerprinted) and will be armed for this job.
    pub skip_armed: bool,
    pub install_active: bool, pub helper_parked: bool, pub helper_exact: bool, pub lease_valid: bool,
    pub pane_index: u32, pub pane_bound: bool,                            // slot+0x1ac, slot+0x1a8!=0 && +0x1a9==1
}

pub fn admit(a: Admission, owner: u32, row: u32) -> Result<(), &'static str> {
    if !a.lease_valid { return Err("portrait lease not valid"); }
    if !a.pane_bound { return Err("visible slot has no bound bust pane"); }
    if a.pane_index >= 0x20 { return Err("visible pane index out of range"); }
    if a.install_active { return Err("INSTALL active"); }
    if !a.helper_parked || !a.helper_exact { return Err("helper not parked/exact"); }
    // run42 observed the gate byte 01 in Training: the copy is prevented only by the scoped skip.
    if a.template_gate != 0 && !a.skip_armed {
        return Err("D template copy gate set and no scoped skip armed; render would rewrite helper D");
    }
    if a.scene_byte & 0xfe == 0x1c { return Err("alternate render path scene"); }
    if a.sequence_pending != 0 || a.sequence_busy != 0 { return Err("native HUD sequence busy"); }
    if a.source_ready == 0 { return Err("native source not ready"); }
    if a.count != 0 || (a.word | 1) == 0xb { return Err("native source busy"); }
    let _ = (owner, row);
    Ok(())
}

/// Explicit original restoration only: never inferred from an absent configuration.
pub fn restore_original(intent: Intent, now_kind: u32, owner: u32, w: RequestWords, table: usize, descriptor: usize) -> bool {
    match intent {
        Intent::Original { original_kind } =>
            now_kind == original_kind && class_sub(original_kind).is_some()
                && w.owned_live(owner) && table != 0 && table == descriptor,
        Intent::Exact => false,
    }
}

/// Exclusive lease rule over the two helper slots (SourceBook facts the caller reads).
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotState {
    pub ready: bool, pub leases: usize, pub same_configuration: bool,
    pub replaceable: bool, pub hat_eligible: bool, pub portrait_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice { Share(usize), Install(usize), Defer }

pub fn choose_slot(slots: [SlotState; 2]) -> Choice {
    for (i, s) in slots.iter().enumerate() {
        if s.ready && s.same_configuration && s.leases == 0 && !s.portrait_pending { return Choice::Share(i); }
    }
    for (i, s) in slots.iter().enumerate() {
        if s.replaceable && s.leases == 0 && s.hat_eligible && !s.portrait_pending { return Choice::Install(i); }
    }
    Choice::Defer
}

/// Hat/item `prepare` must skip a Ready slot while a portrait render is pending on it.
pub fn hat_may_share(portrait_pending: bool) -> bool { !portrait_pending }

/// What the `base+0x32c7404` inline callback sees. Registers come from the `InlineCtx`;
/// `armed_*` from the adapter's atomics (set at issue, cleared on any invalidation);
/// `fresh_*` are re-read from the symbols in the callback; `req_row/req_sub` are `[x20+0]/[x20+4]`.
#[derive(Clone, Copy, Debug)]
pub struct HookView {
    pub x19: usize, pub x20: usize, pub x21: usize,
    pub armed_request: usize, pub armed_entry: usize,
    pub req_row: i32, pub req_sub: i32, pub entry_owner: u32,
    pub fresh_internal: usize, pub fresh_entry: usize, pub fresh_request: usize,
}

/// True only for the one render that belongs to the in-flight, uncontaminated portrait job.
/// A superseded or departed job still qualifies on purpose: its render is still queued and the
/// helper's D block still deserves protection until the queue drains.
pub fn skip_template(v: HookView, in_flight: Option<Job>) -> bool {
    let job = match in_flight { Some(job) => job, None => return false };
    !job.contaminated
        && v.armed_request != 0 && v.x20 == v.armed_request && v.x20 == v.fresh_request
        && v.armed_entry != 0 && v.x19 == v.armed_entry && v.x19 == v.fresh_entry
        && v.x21 == v.fresh_internal
        && v.entry_owner == job.request.row
        && v.req_row == job.request.row as i32 && v.req_sub == job.request.sub as i32
}

/// Completion-time judgement of the helper's D after the render.
pub fn template_outcome(template_gate: u8, skipped: bool) -> Result<(), &'static str> {
    if template_gate != 0 && !skipped {
        return Err("render ran without the scoped skip; helper D assumed templated, re-proof decides");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const ROW: u32 = 2;
    fn idle() -> RequestWords { RequestWords { index: -1, registered: 0, holder: 0 } }
    fn live(row: u32, holder: usize) -> RequestWords { RequestWords { index: table_index(row) as i32, registered: 1, holder } }
    fn req(sub: u32) -> Request { Request { row: ROW, sub } }
    fn words(o: &Ownership, holders: [usize; 3]) -> [RequestWords; 3] {
        let mut w = [idle(); 3];
        for (i, sub) in PORTRAIT_SUBS.iter().copied().enumerate() {
            if o.is_owned(req(sub)) { w[i] = live(ROW, holders[i]); }
        }
        w
    }
    fn q(count: i32, word: u32) -> Queue { Queue { count, word } }
    fn drained(table: usize) -> Completion { Completion { queue: q(0, 0), table, descriptor: table } }
    fn queued(table: usize) -> Completion { Completion { queue: q(1, 0xb), table, descriptor: table } }
    /// Issue, drain and bind one job for `owner`; returns the sub used.
    fn publish(o: &mut Ownership, owner: usize, generation: u64, holders: &mut [usize; 3]) -> u32 {
        let sel = o.select(ROW, words(o, *holders));
        let request = match sel { Select::Request { request, .. } => request, other => panic!("{:?}", other) };
        let i = PORTRAIT_SUBS.iter().position(|s| *s == request.sub).unwrap();
        let desc = 0x7000 + request.sub as usize * 0x100;
        o.issue(Job::new(owner, request, generation, 0x6000)).unwrap();
        holders[i] = desc + 0x28;
        match o.complete(drained(desc), live(ROW, holders[i]), true) {
            Outcome::Bind { owner: b, index } => { assert_eq!((b, index), (owner, 0x14)); }
            other => panic!("{:?}", other),
        }
        o.bound(owner, Binding { request, descriptor: desc });
        request.sub
    }

    #[test]
    fn two_owners_sharing_one_helper_row_never_collide() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        assert_eq!(publish(&mut o, 0, 1, &mut h), 7);
        assert_eq!(publish(&mut o, 1, 1, &mut h), 8);
        assert_eq!(publish(&mut o, 0, 2, &mut h), 9); // 7 still shown by P1 until this bind
        assert_eq!(publish(&mut o, 1, 2, &mut h), 7); // 8 shown by CPU, 9 by P1: only 7 is free
        assert_eq!(publish(&mut o, 0, 3, &mut h), 8);
        for _ in 0..6 {
            let p1 = o.bound[0].unwrap().request.sub; let cpu = o.bound[1].unwrap().request.sub;
            let next = publish(&mut o, 1, 9, &mut h);
            assert_ne!(next, p1); assert_ne!(next, cpu);
        }
        assert_eq!(o.owned_count(), 3);
    }

    #[test]
    fn old_texture_stays_bound_until_the_pane_moves() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        assert_eq!(publish(&mut o, 0, 1, &mut h), 7);
        let request = match o.select(ROW, words(&o, h)) { Select::Request { request, .. } => request, x => panic!("{:?}", x) };
        assert_eq!(request.sub, 8);
        o.issue(Job::new(0, request, 2, 0x7700)).unwrap();
        assert!(matches!(o.select(ROW, words(&o, h)), Select::Refuse(_)));
        assert_eq!(o.bound[0].unwrap().request.sub, 7);
        assert!(matches!(o.complete(queued(0x7800), live(ROW, 1), true), Outcome::Wait));
        // Drained but table pointer unchanged: not a fresh registration of ours.
        assert!(matches!(o.complete(drained(0x7700), live(ROW, 1), true), Outcome::Discard(_)));
        assert_eq!(o.bound[0].unwrap().request.sub, 7);
        assert!(!o.is_owned(req(8)));
        // Fresh issue completes and moves the pane; 7 is then free and 8 protected.
        o.issue(Job::new(0, request, 3, 0x7700)).unwrap();
        assert!(matches!(o.complete(drained(0x7800), live(ROW, 0x7828), true), Outcome::Bind { owner: 0, index: 0x14 }));
        o.bound(0, Binding { request, descriptor: 0x7800 }); h[1] = 0x7828;
        match o.select(ROW, words(&o, h)) { Select::Request { request, first_use } => { assert_eq!(request.sub, 7); assert!(!first_use); } x => panic!("{:?}", x) }
        assert!(o.issue(Job::new(0, req(8), 4, 0)).is_err());
    }

    #[test]
    fn contamination_waits_for_drain_then_discards_and_never_binds() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        assert_eq!(publish(&mut o, 0, 1, &mut h), 7);
        o.issue(Job::new(0, req(8), 2, 0x7700)).unwrap();
        // Fields changed while count 1 / word 0xb: latch, keep the job and the lease.
        assert!(matches!(o.complete(queued(0x7700), idle(), false), Outcome::Wait));
        assert!(o.in_flight.is_some_and(|j| j.contaminated));
        // Fields match again and the queue still has not drained: still waiting.
        assert!(matches!(o.complete(queued(0x7800), live(ROW, 0x7828), true), Outcome::Wait));
        assert!(o.in_flight.is_some());
        // Drained with everything looking registered: discarded, never bound, disowned.
        assert!(matches!(o.complete(drained(0x7800), live(ROW, 0x7828), true), Outcome::Discard(_)));
        assert!(o.in_flight.is_none());
        assert!(!o.is_owned(req(8)));
        assert_eq!(o.bound[0].unwrap().request.sub, 7);
        assert!(matches!(o.select(ROW, words(&o, h)), Select::Request { request: Request { row: 2, sub: 8 }, first_use: true }));
    }

    #[test]
    fn supersession_and_cancellation_wait_for_drain_and_keep_the_bound_texture() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        assert_eq!(publish(&mut o, 0, 1, &mut h), 7);
        let request = req(8);
        o.issue(Job::new(0, request, 2, 0)).unwrap();
        o.supersede(0, 3);
        assert!(matches!(o.complete(queued(0x7800), live(ROW, 0x7828), true), Outcome::Wait));
        assert!(o.in_flight.is_some());
        assert!(matches!(o.complete(drained(0x7800), live(ROW, 0x7828), true), Outcome::Discard(_)));
        assert_eq!(o.bound[0].unwrap().request.sub, 7);
        assert!(o.is_owned(req(8)));
        h[1] = 0x7828;
        match o.select(ROW, words(&o, h)) { Select::Request { request, first_use } => { assert_eq!(request.sub, 8); assert!(!first_use); } x => panic!("{:?}", x) }
        // Cancellation: in-flight superseded, bound record untouched, old texture protected.
        let mut p = Ownership::new(); let mut g = [0; 3];
        assert_eq!(publish(&mut p, 0, 1, &mut g), 7);
        p.issue(Job::new(0, req(8), 2, 0)).unwrap();
        p.cancel_owner(0);
        assert!(p.in_flight.unwrap().superseded);
        assert_eq!(p.bound[0].unwrap().request.sub, 7);
        assert!(p.issue(Job::new(1, req(7), 1, 0)).is_err()); // publisher busy
        assert!(matches!(p.complete(drained(0x7800), live(ROW, 0x7828), true), Outcome::Discard(_)));
        assert_eq!(p.bound[0].unwrap().request.sub, 7);
        assert!(p.issue(Job::new(1, req(7), 1, 0)).is_err()); // still bound
        // Supersession never applies to the other owner's job.
        let mut r = Ownership::new();
        r.issue(Job::new(1, req(7), 1, 0)).unwrap();
        r.supersede(0, 9);
        assert!(!r.in_flight.unwrap().superseded);
    }

    #[test]
    fn departure_and_teardown_keep_the_in_flight_record_until_drain() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        publish(&mut o, 0, 1, &mut h); publish(&mut o, 1, 1, &mut h);
        o.issue(Job::new(0, req(9), 2, 0)).unwrap();
        o.depart();
        assert!(o.in_flight.unwrap().departed);
        assert_eq!(o.owned_count(), 2);
        assert!(o.bound[0].is_some());
        assert!(matches!(o.complete(queued(0x7900), live(ROW, 1), true), Outcome::Wait));
        o.teardown();
        assert_eq!(o.owned_count(), 0); assert_eq!(o.bound, [None; 2]);
        assert!(o.in_flight.is_some());
        assert!(matches!(o.select(ROW, [idle(); 3]), Select::Refuse(_)));
        assert!(matches!(o.complete(drained(0x7900), live(ROW, 0x7928), true), Outcome::Discard(_)));
        assert!(o.in_flight.is_none());
        let mut p = Ownership::new();
        p.issue(Job::new(0, req(7), 1, 0)).unwrap();
        assert!(p.abandon_in_flight().is_some());
        assert!(p.in_flight.is_none());
    }

    #[test]
    fn foreign_requests_are_never_taken_and_lost_ownership_is_reported() {
        let mut o = Ownership::new();
        let w = [live(ROW, 0x55), RequestWords { index: 0x14, registered: 0, holder: 0 }, idle()];
        match o.select(ROW, w) { Select::Request { request, first_use } => { assert_eq!(request.sub, 9); assert!(first_use); } x => panic!("{:?}", x) }
        assert!(matches!(o.select(ROW, [live(ROW, 1), live(ROW, 2), live(ROW, 3)]), Select::Defer(_)));
        let mut p = Ownership::new(); let mut h = [0; 3];
        publish(&mut p, 0, 1, &mut h); publish(&mut p, 1, 1, &mut h);
        let mut w = words(&p, h); w[0] = idle();
        assert_eq!(p.audit(ROW, w), [true, false]);
        assert!(!p.is_owned(req(7))); assert_eq!(p.bound[0], None); assert!(p.bound[1].is_some());
        let mut q = Ownership::new();
        assert!(q.own(Request { row: 3, sub: 7 }));
        assert!(matches!(q.select(ROW, [idle(); 3]), Select::Request { request: Request { row: 2, sub: 7 }, first_use: true }));
        assert!(q.is_owned(Request { row: 3, sub: 7 }));
    }

    #[test]
    fn original_restoration_requires_explicit_intent() {
        let w = live(0, 0x11); let t = 0x1180;
        assert!(restore_original(Intent::Original { original_kind: 0x49 }, 0x49, 0, w, t, t));
        assert!(!restore_original(Intent::Exact, 0x49, 0, w, t, t));
        assert!(!restore_original(Intent::Original { original_kind: 0x49 }, 0x4a, 0, w, t, t));
        assert!(!restore_original(Intent::Original { original_kind: 0x09 }, 0x09, 0, w, t, t));
        assert!(!restore_original(Intent::Original { original_kind: 0x49 }, 0x49, 0, RequestWords { registered: 0, ..w }, t, t));
        assert!(!restore_original(Intent::Original { original_kind: 0x49 }, 0x49, 0, w, 0, 0));
        assert!(!restore_original(Intent::Original { original_kind: 0x49 }, 0x49, 1, w, t, t));
    }

    #[test]
    fn exclusive_lease_rule_never_shares_a_pinned_helper_or_steals() {
        let free_same = SlotState { ready: true, leases: 0, same_configuration: true, replaceable: true, hat_eligible: true, portrait_pending: false };
        let pinned_same = SlotState { leases: 1, replaceable: false, ..free_same };
        let installable = SlotState { ready: false, same_configuration: false, ..free_same };
        let pinned_other = SlotState { same_configuration: false, ..pinned_same };
        let wrong_hat = SlotState { hat_eligible: false, ..installable };
        assert_eq!(choose_slot([free_same, installable]), Choice::Share(0));
        assert_eq!(choose_slot([pinned_same, installable]), Choice::Install(1));
        assert_eq!(choose_slot([pinned_same, pinned_other]), Choice::Defer);
        assert_eq!(choose_slot([pinned_same, wrong_hat]), Choice::Defer);
        assert_eq!(choose_slot([SlotState { portrait_pending: true, ..free_same }, installable]), Choice::Install(1));
        assert!(hat_may_share(false)); assert!(!hat_may_share(true));
    }

    #[test]
    fn admission_tracks_the_native_gates() {
        let ok = Admission { sequence_pending: 0, sequence_busy: 0, source_ready: 1, count: 0, word: 0, template_gate: 0,
            scene_byte: 0x20, skip_armed: false, install_active: false, helper_parked: true, helper_exact: true,
            lease_valid: true, pane_index: 0x12, pane_bound: true };
        assert!(admit(ok, 0, ROW).is_ok());
        assert!(admit(Admission { pane_index: 0x14, ..ok }, 0, ROW).is_ok());
        assert!(admit(Admission { template_gate: 1, skip_armed: true, ..ok }, 0, ROW).is_ok());
        for bad in [Admission { count: 2, word: 0xa, ..ok }, Admission { count: 1, word: 0xb, ..ok }, Admission { word: 0xa, ..ok },
            Admission { sequence_pending: 1, ..ok }, Admission { sequence_busy: 1, ..ok }, Admission { source_ready: 0, ..ok },
            Admission { template_gate: 1, ..ok }, Admission { scene_byte: 0x1d, ..ok }, Admission { helper_parked: false, ..ok },
            Admission { helper_exact: false, ..ok }, Admission { install_active: true, ..ok }, Admission { lease_valid: false, ..ok },
            Admission { pane_bound: false, ..ok }, Admission { pane_index: 0x20, ..ok }] {
            assert!(admit(bad, 0, ROW).is_err());
        }
    }

    #[test]
    fn skip_only_for_the_exact_armed_uncontaminated_render() {
        let job = Job::new(0, Request { row: 2, sub: 7 }, 1, 0);
        let view = HookView { x19: 0x11ef_0000, x20: 0x0011_e1f2_3800, x21: 0x0011_811a_1800,
            armed_request: 0x0011_e1f2_3800, armed_entry: 0x11ef_0000, req_row: 2, req_sub: 7, entry_owner: 2,
            fresh_internal: 0x0011_811a_1800, fresh_entry: 0x11ef_0000, fresh_request: 0x0011_e1f2_3800 };
        assert!(skip_template(view, Some(job)));
        assert!(!skip_template(view, None));
        assert!(!skip_template(view, Some(Job { contaminated: true, ..job })));
        // Superseded or departed jobs are still queued renders of the helper: protect D until drain.
        assert!(skip_template(view, Some(Job { superseded: true, ..job })));
        assert!(skip_template(view, Some(Job { departed: true, ..job })));
        for bad in [
            HookView { armed_request: 0, ..view },
            HookView { x20: 0x0011_e100_0000, ..view },
            HookView { armed_request: 0x0011_e100_0000, ..view },
            HookView { x19: 0x11ee_0000, ..view },
            HookView { armed_entry: 0x11ee_0000, ..view },
            HookView { fresh_entry: 0x11ef_1000, ..view },
            HookView { x21: 0x0011_8100_0000, ..view },
            HookView { req_row: 0, ..view },
            HookView { req_sub: 8, ..view },
            HookView { entry_owner: 3, ..view },
            HookView { fresh_request: 0x0011_e1f2_3900, ..view },
        ] {
            assert!(!skip_template(bad, Some(job)));
        }
        assert!(!skip_template(view, Some(Job { request: Request { row: 2, sub: 8 }, ..job })));
        assert!(template_outcome(0, false).is_ok());
        assert!(template_outcome(1, true).is_ok());
        assert!(template_outcome(1, false).is_err());
    }

    /// hud59 P3: a superseded render registers (h,8) and is not bound, so (h,8) is owned and is
    /// the table occupant. The superseding render reuses it; its fresh registration rewrites the
    /// same descriptor and must still bind once the retarget clear was seen.
    #[test]
    fn reuse_of_the_table_occupant_binds_after_an_observed_clear() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        assert_eq!(publish(&mut o, 0, 1, &mut h), 7);
        o.issue(Job::new(0, req(8), 2, 0x7700)).unwrap();
        o.supersede(0, 3);
        assert!(matches!(o.complete(drained(0x7800), live(ROW, 0x7828), true), Outcome::Discard(_)));
        h[1] = 0x7828;
        let request = match o.select(ROW, words(&o, h)) { Select::Request { request, .. } => request, x => panic!("{:?}", x) };
        assert_eq!(request, req(8));
        o.issue(Job::new(0, request, 3, 0x7800)).unwrap();
        assert!(matches!(o.complete(queued(0x7800), idle(), true), Outcome::Wait));
        assert!(matches!(o.complete(drained(0x7800), live(ROW, 0x7829), true), Outcome::Bind { owner: 0, index: 0x14 }));
        // Without the observed clear the same drained words are still not a fresh registration.
        o.bound(0, Binding { request, descriptor: 0x7800 });
        o.issue(Job::new(1, req(9), 1, 0x7900)).unwrap();
        assert!(matches!(o.complete(drained(0x7900), live(ROW, 1), true), Outcome::Discard(_)));
    }

    /// hud59 P4: a verified original rebind releases the custom binding without disowning it.
    #[test]
    fn verified_original_rebind_unbinds_without_disowning() {
        let mut o = Ownership::new(); let mut h = [0; 3];
        assert_eq!(publish(&mut o, 0, 1, &mut h), 7);
        o.unbind(0);
        assert!(o.bound[0].is_none());
        assert!(o.is_owned(req(7)));
        match o.select(ROW, words(&o, h)) { Select::Request { request, first_use } => { assert_eq!(request.sub, 7); assert!(!first_use); } x => panic!("{:?}", x) }
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    fn binding(row: u32, sub: u32) -> Binding { Binding { request: Request { row, sub }, descriptor: 0x7000 + sub as usize } }

    #[test]
    fn one_bust_row_per_owner() {
        let none: [Option<Binding>; VISIBLE_OWNERS] = [None, None];
        assert!(row_free_for(&none, 0, 2));
        assert!(row_free_for(&none, 1, 2));
        // P1 bound on helper row 2: the CPU may not render on row 2, but may on row 3.
        let p1_on_2 = [Some(binding(2, 7)), None];
        assert!(row_free_for(&p1_on_2, 0, 2));   // the same owner replacing its own bust
        assert!(!row_free_for(&p1_on_2, 1, 2));
        assert!(row_free_for(&p1_on_2, 1, 3));
        // Both bound on distinct rows: each may only keep its own row.
        let split = [Some(binding(2, 7)), Some(binding(3, 7))];
        assert!(row_free_for(&split, 0, 2));
        assert!(!row_free_for(&split, 0, 3));
        assert!(row_free_for(&split, 1, 3));
        assert!(!row_free_for(&split, 1, 2));
        // The guard composes with the existing bound-request exclusion in `select`.
        let mut o = Ownership::new();
        o.bound(0, binding(2, 7));
        assert!(matches!(o.select(2, [RequestWords { index: -1, registered: 0, holder: 0 }; 3]),
            Select::Request { request: Request { row: 2, sub: 8 }, first_use: true }));
        assert!(!row_free_for(&o.bound, 1, 2));
    }
}
/// A helper avoided by this owner cannot satisfy or retain its pending want.
pub fn slot_may_install(avoid: Option<u32>, slot: u32) -> bool { avoid != Some(slot) }

/// Validate the part index before the adapter reads its child pointer.
pub fn part_offset(index: i32) -> Option<usize> {
    (0..8).contains(&index).then(|| index as usize * 0xd90 + 0x330)
}

/// Native portrait stores are admitted only in qualified practice scenes.
pub fn may_write_scene(melee: u8, mode: u32) -> bool {
    matches!((melee, mode), (0x20, 0x04040000) | (0x0d, 0x02010000))
}

#[cfg(test)]
mod native_admission_tests {
    use super::*;
    #[test]
    fn render_preset_does_not_change_request_identity_or_admit_foreign_work() {
        for sub in PORTRAIT_SUBS {
            let request = Request { row: 2, sub };
            let job = Job::new(0, request, 1, 0);
            for (kind, preset) in [(0x48, 4), (0x4a, 5), (0x49, 6)] {
                assert_eq!(render_preset(request, sub, kind, Some(job)), Some(preset));
                assert_eq!(job.request, request);
            }
            assert_eq!(render_preset(request, sub, 0, Some(job)), None);
            assert_eq!(render_preset(request, sub, 0x48, None), None);
            assert_eq!(render_preset(request, sub + 1, 0x48, Some(job)), None);
            assert_eq!(render_preset(request, sub, 0x48, Some(Job { contaminated: true, ..job })), None);
            assert_eq!(render_preset(Request { row: 3, sub }, sub, 0x48, Some(job)), None);
            // These jobs still drain, so their render framing remains consistent.
            assert_eq!(render_preset(request, sub, 0x48, Some(Job { departed: true, superseded: true, ..job })), Some(4));
        }
        for request in [Request { row: 8, sub: 7 }, Request { row: 2, sub: 4 }] {
            assert_eq!(render_preset(request, request.sub, 0x48, Some(Job::new(0, request, 1, 0))), None);
        }
    }
    #[test]
    fn invalid_part_indices_never_produce_an_address_offset() {
        for index in [i32::MIN, -1, 8, i32::MAX] { assert_eq!(part_offset(index), None); }
        assert_eq!(part_offset(0), Some(0x330));
        assert_eq!(part_offset(7), Some(0x6220));
    }
    #[test]
    fn avoided_rows_do_not_satisfy_or_pin_a_portrait_want() {
        assert!(!slot_may_install(Some(2), 2));
        assert!(slot_may_install(Some(2), 3));
        for slot in 0..8 { assert!(slot_may_install(None, slot)); }
    }
    #[test]
    fn native_stores_reject_match_and_unqualified_scene_modes() {
        assert!(may_write_scene(0x20, 0x04040000));
        assert!(may_write_scene(0x0d, 0x02010000));
        for (melee, mode) in [(0x0d,0x07010000),(0x0d,0x07010001),(1,0x01010000),(0x20,0x02010000)] {
            assert!(!may_write_scene(melee, mode));
        }
    }
}
