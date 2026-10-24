//! Deferred replay resource identity. The existing Training service is the only native seam.
use super::*;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc},
};
use training_mod_consts::mii::{
    draft::Library,
    lifetime::Configuration,
    native_values::RowIds,
    replay::{RawMii, ReplayMiis, Selection, SelectionSource, Target},
};

const MAX_PENDING: usize = training_mod_macros::MAX_RETAINED_CAPTURES * 8;
static WORK: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State {
    pending: VecDeque::new(),
    inflight: None,
});
type Reply = mpsc::Sender<Result<Configuration, String>>;
struct Job {
    raw: RawMii,
    selected: Option<(u64, u64, Reply)>,
}
struct Inflight {
    job: Job,
    configuration: Configuration,
    result: mpsc::Receiver<Result<Arc<Library>, String>>,
}
struct State {
    pending: VecDeque<Job>,
    inflight: Option<Inflight>,
}
fn generation() -> u64 {
    mii_saved_alpha::current_generation(1)
}
fn current(job: &Job) -> bool {
    job.selected.as_ref().is_none_or(|(epoch, revision, _)| {
        *epoch == generation() && *revision == crate::replay_capture::selection_revision()
    })
}
fn finish(job: Job, result: Result<Configuration, String>) {
    if let Some((_, _, reply)) = job.selected {
        let _ = reply.send(result);
    } else if let Err(error) = result {
        trace!("[MiiReplay] deferred import unavailable: {error}; durable raw metadata retained");
    }
}

/// Called only after the companion was committed. This bounded queue is an optimization;
/// the durable association remains authoritative and selected captures can retry after restart.
pub(crate) fn committed(miis: ReplayMiis) {
    let mut s = STATE.lock().unwrap();
    for raw in miis.entries() {
        if s.pending.len() == MAX_PENDING {
            break;
        }
        s.pending.push_back(Job {
            raw: *raw,
            selected: None,
        });
    }
    WORK.store(
        !s.pending.is_empty() || s.inflight.is_some(),
        Ordering::Release,
    );
}
pub(crate) fn resolve(
    raw: RawMii,
    revision: u64,
) -> Result<mpsc::Receiver<Result<Configuration, String>>, String> {
    if !mii_ordinary_cpu::enabled() {
        return Err("Replay Mii loading is unavailable in this build".into());
    }
    let (reply, receive) = mpsc::channel();
    let mut s = STATE.lock().unwrap();
    if s.pending.len() == MAX_PENDING {
        return Err("Mii import queue is busy; reopen this capture".into());
    }
    s.pending.push_front(Job {
        raw,
        selected: Some((generation(), revision, reply)),
    });
    WORK.store(true, Ordering::Release);
    Ok(receive)
}
pub(crate) fn selected(target: Target, revision: u64) -> Selection {
    Selection {
        source: SelectionSource::Clip,
        generation: generation(),
        revision,
        target,
    }
}
/// Atomic-only query: safe while the existing ordinary CPU STATE lock is held.
pub(super) fn revision(source: SelectionSource) -> u64 {
    match source {
        SelectionSource::Clip => crate::replay_capture::selection_revision(),
        SelectionSource::NamedMacro => crate::training::named_macros::selection_revision(),
    }
}
fn named_selection(target: Target) -> Selection {
    Selection { source: SelectionSource::NamedMacro, generation: generation(),
        revision: revision(SelectionSource::NamedMacro), target }
}
pub(crate) fn reacknowledge_named(target: Target) {
    if target.configuration().is_some() {
        mii_ordinary_cpu::request_reacknowledge(named_selection(target));
    }
}
pub(crate) fn named_ready(target: Target, kind: u32) -> bool {
    mii_ordinary_cpu::replay_ready(named_selection(target), kind)
}
pub(crate) fn named_refusal(target: Target) -> Option<&'static str> {
    mii_ordinary_cpu::replay_refusal(named_selection(target))
}
pub(crate) fn request_named(target: Target) -> bool {
    if mii_ordinary_cpu::replay_catalog().is_some() {
        mii_ordinary_cpu::request_replay(named_selection(target))
    } else if target.configuration().is_none() { request_cpu_kind(target.kind()) }
    else { false }
}
pub(crate) fn refusal(target: Target, revision: u64) -> Option<&'static str> {
    mii_ordinary_cpu::replay_refusal(selected(target, revision))
}
pub(crate) fn ready(target: Target, kind: u32) -> bool {
    if target.configuration().is_some() {
        mii_ordinary_cpu::replay_ready(
            selected(target, crate::replay_capture::selection_revision()),
            kind,
        )
    } else {
        target.ready(kind, mii_ordinary_cpu::replay_current())
    }
}
pub(crate) fn request(target: Target, revision: u64) -> bool {
    if mii_ordinary_cpu::replay_catalog().is_some() {
        mii_ordinary_cpu::request_replay(selected(target, revision))
    } else if target.configuration().is_none() {
        request_cpu_kind(target.kind())
    } else {
        false
    }
}

pub(super) unsafe fn service(entry: *const u8, owner: u32) {
    if mii_backend::cpu()!=Some(owner)
        || !WORK.load(Ordering::Acquire)
        || !mii_ordinary_cpu::enabled()
    {
        return;
    }
    let completed = {
        let mut s = STATE.lock().unwrap();
        let result = s.inflight.as_ref().map(|i| i.result.try_recv());
        match result {
            Some(Ok(result)) => Some((s.inflight.take().unwrap(), result)),
            Some(Err(mpsc::TryRecvError::Disconnected)) => Some((
                s.inflight.take().unwrap(),
                Err("Mii import writer disconnected".into()),
            )),
            _ => None,
        }
    };
    if let Some((inflight, result)) = completed {
        let result = result.map(|library| {
            mii_ordinary_player::refresh_library(&library);
            mii_ordinary_cpu::refresh_library(&library);
            inflight.configuration
        });
        let result = if current(&inflight.job) {
            result
        } else {
            Err("Replay selection retired".into())
        };
        finish(inflight.job, result);
    }
    let mut s = STATE.lock().unwrap();
    if s.inflight.is_some() {
        return;
    }
    while s.pending.front().is_some_and(|job| !current(job)) {
        finish(
            s.pending.pop_front().unwrap(),
            Err("Replay selection retired".into()),
        );
    }
    if s.pending.is_empty() {
        WORK.store(false, Ordering::Release);
        return;
    }
    drop(s);
    let Some(catalog) = mii_ordinary_cpu::replay_catalog() else {
        return;
    };
    // No scans or retries before readiness. No locks/native pointers enter the file worker.
    if !crate::common::is_ready_go()
        || INSTALL.active()
        || menu::captures_input()
        || lobby::native_ui_busy()
        || button_config::has_pending_request()
        || training::save_states::is_loading()
        || training::recording_or_playback()
        || crate::replay_preview::occupied()
        || !mii_backend::ready()
        || !(ready_single_mii(entry, owner).is_some() || ready_forms(entry, owner).is_some())
    {
        return;
    }
    let Some(job) = STATE.lock().unwrap().pending.pop_front() else {
        return;
    };
    use crate::mii_native_access::{lookup_row_id, HAT_TABLE, OUTFIT_TABLE};
    let tokens = job.raw.tokens();
    let resolved = (|| {
        let base = BASE.load(Ordering::Acquire);
        let rows = RowIds {
            outfit: lookup_row_id(base, OUTFIT_TABLE, tokens.outfit)?,
            hat: lookup_row_id(base, HAT_TABLE, tokens.hat)?,
        };
        job.raw
            .resolve(rows)
            .map_err(|_| "Replay Mii resource identity did not resolve")
    })();
    match resolved {
        Ok(configuration) => {
            if !current(&job) {
                finish(job, Err("Replay selection retired".into()));
                return;
            }
            let (reply, result) = mpsc::channel();
            match crate::replay_store::import_mii(configuration, catalog, reply) {
                Ok(()) => {
                    STATE.lock().unwrap().inflight = Some(Inflight {
                        job,
                        configuration,
                        result,
                    })
                }
                Err(error) => finish(job, Err(error)),
            }
        }
        Err(error) => finish(job, Err(error.into())),
    }
}
