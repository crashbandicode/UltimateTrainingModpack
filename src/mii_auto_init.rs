//! Current combined alpha: one data-only catalog worker, then current-callback capture.
use super::*;
use std::sync::{mpsc, Arc};
use training_mod_consts::mii::{
    auto_init::{Initialization, Token},
    inventory::Snapshot,
};

// Completed initialization costs one atomic load per visible entry callback.
static PENDING: AtomicBool = AtomicBool::new(true);
/// Safe to query under a role STATE lock; never takes the coordinator mutex.
pub(super) fn pending() -> bool {
    PENDING.load(Ordering::Acquire)
}
static STATE: Mutex<State> = Mutex::new(State {
    init: Initialization::empty(),
    result: None,
});
struct State {
    init: Initialization,
    result: Option<mpsc::Receiver<(Token, Arc<Snapshot>)>>,
}
fn token() -> Token {
    Token {
        generations: [
            mii_saved_alpha::current_generation(0),
            mii_saved_alpha::current_generation(1),
        ],
        opening: mii_saved_alpha::menu_epoch(),
    }
}
pub(super) fn cancel() {
    let mut s = STATE.lock().unwrap();
    s.init.cancel();
    s.result = None;
    PENDING.store(false, Ordering::Release);
}
pub(super) fn reset() {
    let mut s = STATE.lock().unwrap();
    s.init.reset();
    s.result = None;
    PENDING.store(true, Ordering::Release);
}
unsafe fn idle() -> bool {
    mii_backend::ready()
        && crate::common::is_ready_go()
        && !menu::captures_input()
        && !lobby::native_ui_busy()
        && !button_config::has_pending_request()
        && !INSTALL.active()
        && !training::save_states::is_loading()
        && !training::recording_or_playback()
        && !crate::replay_preview::occupied()
        && STATES.iter().all(|s| {
            let s = s.lock().unwrap();
            s.active.is_none() && s.pending.is_none()
        })
}
pub(super) unsafe fn service(entry: *const u8, owner: u32) {
    let Some(logical)=mii_backend::current().and_then(|l|l.logical(owner)) else{return;};
    if logical > 1
        || !pending()
        || !mii_ordinary_player::enabled()
        || !mii_ordinary_cpu::enabled()
    {
        return;
    }
    let eligible = original_kind(Role::Player).is_some()
        && original_kind(Role::Cpu).is_some()
        && scope(Role::Player).is_some_and(|s| s.owner == 0)
        && idle()
        && (ready_single_mii(entry, owner).is_some() || ready_forms(entry, owner).is_some());
    let mut s = STATE.lock().unwrap();
    let current = token();
    if let Some(result) = s.result.as_ref() {
        // Keep the completed owned values until a current callback is eligible to arm capture.
        if !eligible {
            return;
        }
        let received = result.try_recv();
        match received {
            Ok((started, catalog)) => {
                s.result = None;
                let arm = s.init.finish(started, current, catalog.is_complete());
                PENDING.store(s.init.pending(), Ordering::Release);
                drop(s);
                if arm {
                    let player = mii_ordinary_player::catalog_ready(
                        Arc::clone(&catalog),
                        started.generations[0],
                        started.opening,
                    );
                    let cpu = mii_ordinary_cpu::catalog_ready(
                        catalog,
                        started.generations[1],
                        started.opening,
                    );
                    trace!("[MiiAutoInit] catalog ready; capture_armed={:?} generations={:?} opening={}", [player,cpu], started.generations, started.opening);
                } else {
                    trace!(
                        "[MiiAutoInit] catalog refused: incomplete or retired; no automatic retry"
                    );
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                s.init.cancel();
                s.result = None;
                PENDING.store(false, Ordering::Release);
                drop(s);
                trace!("[MiiAutoInit] catalog worker unavailable; no automatic retry");
            }
        }
        return;
    }
    let Some(started) = s.init.observe(current, logical as u32, eligible) else {
        return;
    };
    let (send, receive) = mpsc::channel();
    s.result = Some(receive);
    drop(s);
    // No native pointer, native call, App or adapter lock crosses this boundary.
    let worker = std::thread::Builder::new()
        .name("mii-catalog".into())
        .stack_size(0x20000)
        .spawn(move || {
            let catalog = Arc::new(crate::replay_store::mii_catalog());
            let _ = send.send((started, catalog));
        });
    if let Err(error) = worker {
        let mut s = STATE.lock().unwrap();
        // A failed old spawn cannot cancel a new lifetime's worker.
        let _ = s.init.finish(started, token(), false);
        if !s.init.pending() {
            s.result = None;
            PENDING.store(false, Ordering::Release);
        }
        drop(s);
        trace!("[MiiAutoInit] catalog worker launch failed: {error}; no automatic retry");
    } else {
        trace!(
            "[MiiAutoInit] catalog read started generations={:?} opening={}",
            started.generations,
            started.opening
        );
    }
}
