//! Revision 31: hang tracing for the console.
//!
//! The revision 30 Switch smoke froze on a loading screen without a panic or a crash report, so
//! the diagnostics file ended at the last breadcrumb the game thread happened to write. This
//! module records, without locks, how often and how recently each modpack hook ran and whether
//! one is still executing, and a heartbeat thread writes one summary line every few seconds.
//! A frozen console therefore keeps writing until the operator powers it off, and the last
//! heartbeat shows which hooks stopped advancing and which one, if any, never returned.
//!
//! Probes touch only atomics; the heartbeat reads them and the game-mode tuple and calls
//! `logging::diagnostic`, which itself only `try_lock`s the diagnostics file.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// One instrumented hook: call count, time of the last call and, between `enter` and the guard's
/// drop, the time the current call started plus one caller-chosen detail word.
pub struct Probe {
    name: &'static str,
    count: AtomicU64,
    last_ms: AtomicU64,
    active_since_ms: AtomicU64,
    detail: AtomicU64,
}

impl Probe {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            count: AtomicU64::new(0),
            last_ms: AtomicU64::new(0),
            active_since_ms: AtomicU64::new(0),
            detail: AtomicU64::new(0),
        }
    }

    /// Record one call of a hook that does not need an active window.
    pub fn hit(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.last_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// Record one call and mark the probe active until the returned guard drops.
    pub fn enter(&self, detail: u64) -> ProbeGuard<'_> {
        self.hit();
        self.detail.store(detail, Ordering::Relaxed);
        self.active_since_ms.store(now_ms().max(1), Ordering::Release);
        ProbeGuard { probe: self }
    }

    fn summary(&self, now: u64) -> String {
        let count = self.count.load(Ordering::Relaxed);
        let last = self.last_ms.load(Ordering::Relaxed);
        let active = self.active_since_ms.load(Ordering::Acquire);
        let age = if last == 0 {
            String::from("never")
        } else {
            format!("{}ms", now.saturating_sub(last))
        };
        if active == 0 {
            format!("{}={}/{}", self.name, count, age)
        } else {
            format!(
                "{}={}/{} ACTIVE {}ms detail={:#x}",
                self.name,
                count,
                age,
                now.saturating_sub(active),
                self.detail.load(Ordering::Relaxed)
            )
        }
    }
}

pub struct ProbeGuard<'a> {
    probe: &'a Probe,
}

impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        self.probe.active_since_ms.store(0, Ordering::Release);
    }
}

/// Per-frame UI pass anchor (UI job thread).
pub static UI_PASS: Probe = Probe::new("ui_pass");
/// Manager-insert anchor (main thread), one call per newly registered layout.
pub static UI_REGISTER: Probe = Probe::new("ui_register");
/// The modpack's `ui::tick` body entered from the per-frame pass.
pub static TICK_PASS: Probe = Probe::new("tick_pass");
/// The modpack's `ui::tick` body entered from a layout registration.
pub static TICK_REGISTER: Probe = Probe::new("tick_register");
/// Fighter entry service callback (game thread, per entry per frame).
pub static ROSTER_SERVICE: Probe = Probe::new("roster_service");
/// Fighter entry allocation, reload and destruction boundaries.
pub static ALLOCATION: Probe = Probe::new("alloc");
pub static RELOAD: Probe = Probe::new("reload");
pub static DESTROY: Probe = Probe::new("destroy");
/// Layout archive decompression hook (the Training and waiting UI injection point).
pub static LAYOUT_ARC: Probe = Probe::new("layout_arc");
/// Skyline NRO load callback (fighter and common modules load here during a scene load).
pub static NRO_LOAD: Probe = Probe::new("nro_load");

static PROBES: [&Probe; 10] = [
    &UI_PASS,
    &UI_REGISTER,
    &TICK_PASS,
    &TICK_REGISTER,
    &ROSTER_SERVICE,
    &ALLOCATION,
    &RELOAD,
    &DESTROY,
    &LAYOUT_ARC,
    &NRO_LOAD,
];

/// Bounded per-session line budgets for the chattier trace lines.
pub static REGISTER_LINES: AtomicU32 = AtomicU32::new(0);
pub static NRO_LINES: AtomicU32 = AtomicU32::new(0);

/// True while `counter` is below `limit`; each call consumes one unit of the budget.
pub fn within_budget(counter: &AtomicU32, limit: u32) -> bool {
    counter.fetch_add(1, Ordering::Relaxed) < limit
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Start the heartbeat thread. Failures only cost the heartbeat; the probes still count.
pub fn start() {
    let started = now_ms();
    let spawned = std::thread::Builder::new()
        .name("practice-heartbeat".into())
        .stack_size(0x10000)
        .spawn(move || {
            let mut beat = 0u64;
            loop {
                std::thread::sleep(HEARTBEAT_INTERVAL);
                beat += 1;
                let now = now_ms();
                // Revision 33: housekeeping off the game threads — release inflated layout archives
                // the game finished copying (training::ui).
                crate::training::ui::release_stale_archives(now);
                let probes: Vec<String> = PROBES.iter().map(|probe| probe.summary(now)).collect();
                crate::logging::diagnostic(format_args!(
                    "[Heartbeat] n={} up={}s mode={:?} {}",
                    beat,
                    now.saturating_sub(started) / 1000,
                    crate::common::game_mode::current(),
                    probes.join(" ")
                ));
            }
        });
    match spawned {
        Ok(_) => crate::logging::diagnostic(format_args!(
            "[Heartbeat] thread started interval={}s probes={}",
            HEARTBEAT_INTERVAL.as_secs(),
            PROBES.len()
        )),
        Err(error) => crate::logging::diagnostic(format_args!(
            "[Heartbeat] thread unavailable: {error}"
        )),
    }
}
