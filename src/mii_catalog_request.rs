//! One shared catalog read for menu open and lifetime auto-init.
//! The game thread only queues and later applies an owned snapshot.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use training_mod_consts::mii::{
    catalog_request::{Effect, Kind, Live, Requests, Step, Verdict},
    inventory::Snapshot,
};

static PENDING: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Broker> = Mutex::new(Broker::empty());

struct Broker {
    policy: Requests,
    reply: Option<mpsc::Receiver<Snapshot>>,
    flight_serial: u64,
    held_editor: Option<(Snapshot, u64)>,
}

impl Broker {
    const fn empty() -> Self {
        Self {
            policy: Requests::new(),
            reply: None,
            flight_serial: 0,
            held_editor: None,
        }
    }

    fn busy(&self) -> bool {
        self.held_editor.is_some() || self.policy.pending()
    }
}

fn live() -> Live {
    Live {
        menu_active: crate::common::menu::quick_menu_active(),
        opening: super::mii_saved_alpha::menu_epoch(),
        owners: [
            super::mii_saved_alpha::current_generation(0),
            super::mii_saved_alpha::current_generation(1),
        ],
        auto: reading_token(),
    }
}

fn reading_token() -> Option<training_mod_consts::mii::auto_init::Token> {
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    {
        super::mii_auto_init::reading_token()
    }
    #[cfg(not(feature = "mii-ordinary-cpu-alpha"))]
    {
        None
    }
}

pub(crate) fn request(kind: Kind) {
    let mut broker = STATE.lock().unwrap();
    let effect = broker.policy.request(kind);
    broker.drive(effect);
    PENDING.store(broker.busy(), Ordering::Release);
}

pub(crate) fn close_opening(opening: u64) {
    let mut broker = STATE.lock().unwrap();
    let restart_auto = broker.policy.close_menu(opening);
    if broker
        .held_editor
        .as_ref()
        .is_some_and(|(_, held)| *held == opening)
    {
        broker.held_editor = None;
    }
    PENDING.store(broker.busy(), Ordering::Release);
    drop(broker);
    // A quick close can cancel the menu read that superseded automatic capture.
    // Restart through the normal eligible native callback, never the stale reply.
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    if restart_auto {
        super::mii_auto_init::reset();
    }
    #[cfg(not(feature = "mii-ordinary-cpu-alpha"))]
    let _ = restart_auto;
}

pub(super) fn retire() {
    let mut broker = STATE.lock().unwrap();
    broker.policy.retire();
    broker.held_editor = None;
    PENDING.store(broker.busy(), Ordering::Release);
}

pub(super) fn service() {
    if !PENDING.load(Ordering::Acquire) {
        return;
    }
    let (held, ready) = {
        let mut broker = STATE.lock().unwrap();
        let held = broker.held_editor.take();
        let ready = broker.poll();
        PENDING.store(broker.busy(), Ordering::Release);
        (held, ready)
    };
    if let Some((snapshot, opening)) = held {
        retain_editor(snapshot, opening);
    }
    if let Some((verdict, snapshot)) = ready {
        apply(verdict, snapshot);
    }
}

impl Broker {
    fn poll(&mut self) -> Option<(Verdict, Snapshot)> {
        if let Some(reply) = self.reply.as_ref() {
            match reply.try_recv() {
                Ok(snapshot) => {
                    self.reply = None;
                    let effect = self.policy.complete(self.flight_serial, live());
                    let verdict = effect.verdict;
                    self.drive(effect);
                    return Some((verdict, snapshot));
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.reply = None;
                    let effect = self.policy.failed(live());
                    self.drive(effect);
                    return None;
                }
            }
        }
        if self.policy.needs_retry() {
            let effect = self.policy.retry(live());
            self.drive(effect);
        }
        None
    }

    fn drive(&mut self, mut effect: Effect) {
        for _ in 0..2 {
            self.abandon(&effect);
            if !matches!(effect.step, Step::Submit(_)) {
                Self::note(&effect);
                return;
            }
            effect = self.enqueue();
        }
        self.abandon(&effect);
        Self::note(&effect);
    }

    fn enqueue(&mut self) -> Effect {
        let (sender, receiver) = mpsc::sync_channel(1);
        match crate::replay_store::request_mii_catalog(sender) {
            crate::replay_store::CatalogQueue::Queued => {
                self.flight_serial = self.policy.serial();
                self.reply = Some(receiver);
                Effect::quiet(Step::Wait)
            }
            crate::replay_store::CatalogQueue::Full => {
                drop(receiver);
                self.policy.rejected_full()
            }
            crate::replay_store::CatalogQueue::Unavailable => {
                drop(receiver);
                self.policy.failed(live())
            }
        }
    }

    fn abandon(&self, effect: &Effect) {
        abandon_auto(effect.dropped);
        if let Some(kind) = effect.finished {
            let applied = matches!(
                (kind, effect.verdict),
                (Kind::Auto(finished), Verdict::Auto(applied)) if finished == applied
            );
            if !applied {
                abandon_auto(Some(kind));
            }
        }
    }

    fn note(effect: &Effect) {
        if matches!(effect.step, Step::Unavailable) {
            trace!("[MiiCatalog] saved-Mii catalog unavailable");
        }
    }
}

fn abandon_auto(kind: Option<Kind>) {
    let Some(Kind::Auto(token)) = kind else {
        return;
    };
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    super::mii_auto_init::abandon(token);
    #[cfg(not(feature = "mii-ordinary-cpu-alpha"))]
    {
        let _ = token;
    }
}

fn apply(verdict: Verdict, snapshot: Snapshot) {
    match verdict {
        Verdict::Discard => {}
        Verdict::Menu { opening, owners } => {
            if super::mii_saved_alpha::accept_catalog(Arc::new(snapshot), opening, owners) {
                STATE.lock().unwrap().policy.menu_armed(opening);
            }
        }
        Verdict::Editor { opening } => retain_editor(snapshot, opening),
        Verdict::Auto(token) => {
            #[cfg(feature = "mii-ordinary-cpu-alpha")]
            super::mii_auto_init::deliver(token, Arc::new(snapshot));
            #[cfg(not(feature = "mii-ordinary-cpu-alpha"))]
            {
                let _ = (token, snapshot);
            }
        }
    }
}

fn retain_editor(snapshot: Snapshot, opening: u64) {
    #[cfg(feature = "mii-editor-alpha")]
    {
        match crate::mii_editor_bridge::mount_opened_catalog(snapshot, opening) {
            crate::mii_editor_bridge::OpenedCatalog::Shown
            | crate::mii_editor_bridge::OpenedCatalog::Dropped => {}
            crate::mii_editor_bridge::OpenedCatalog::Later(snapshot) => {
                let mut broker = STATE.lock().unwrap();
                if broker.policy.allows_editor(opening, &live()) {
                    broker.held_editor = Some((snapshot, opening));
                }
                PENDING.store(broker.busy(), Ordering::Release);
            }
        }
    }
    #[cfg(not(feature = "mii-editor-alpha"))]
    {
        let _ = (snapshot, opening);
    }
}
