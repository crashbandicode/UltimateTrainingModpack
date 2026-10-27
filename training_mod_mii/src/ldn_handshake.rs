//! Guest-side Local Wireless round handshake, as owned values only.
//!
//! The native network service keeps a per-round station acknowledgement mask
//! and a "round resolved" byte. Native waiting practice consumes
//! `resolved != 0 && ack != 0` itself and later queues its own match-start
//! event. While the editor owns the scene that consumer does not exist, so the
//! bridge observes the same two values and reports at most one typed committed
//! edge per bound editor session. This module never names an event kind,
//! never fabricates a queue element, never clears a flag and never decides
//! what native code does next; the bridge hands the still-set state back
//! through its existing interruption path and native practice makes its own
//! event.

/// Which bound session a sample belongs to: the freshly rebound service
/// address and the Local Wireless outer lifetime generation. Both are
/// comparison tokens only; nothing here dereferences them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub service: usize,
    pub generation: u64,
}

/// One callback's fresh reads. `identity` is `None` when the service chain or
/// the owning lobby generation could not be rebound on this callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub identity: Option<Identity>,
    /// The per-round acknowledgement mask (`u32`).
    pub ack: u32,
    /// The round-resolved byte (`u8`).
    pub resolved: u8,
}

impl Sample {
    /// No same-session service: nothing can be observed from this sample.
    pub const UNBOUND: Self = Self { identity: None, ack: 0, resolved: 0 };

    /// The exact native consumer predicate: the round resolved with at least
    /// one acknowledgement recorded.
    pub const fn committed(self) -> bool { self.resolved != 0 && self.ack != 0 }

    /// Resolved without any acknowledgement: the consumer's abort branch.
    /// Never Begin.
    pub const fn aborted(self) -> bool { self.resolved != 0 && self.ack == 0 }
}

/// What one fresh sample meant to the session's detector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    /// No same-session service to observe; any recorded baseline is dropped.
    Unbound,
    /// A genuine `resolved == 0` reading is now recorded under this identity.
    Baseline,
    /// Still the recorded baseline; the round has not resolved.
    Waiting,
    /// Resolved before any baseline was recorded in this session, or under a
    /// different service or lobby generation than the baseline. Cannot fire.
    StaleHigh,
    /// Resolved with an empty mask after a baseline. Never Begin; the baseline
    /// stays armed so one non-atomic read cannot disarm the session.
    Aborted,
    /// The one committed edge of this session.
    Committed,
    /// The committed edge was already reported under this identity.
    Held,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Unarmed,
    Armed(Identity),
    Committed(Identity),
}

/// Per-editor-session edge detector. It is constructed unarmed with the
/// session, consulted only while that session may still be interrupted, and
/// ended with it. A new session always starts from a fresh value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handshake {
    state: State,
}

impl Default for Handshake {
    fn default() -> Self { Self::new() }
}

impl Handshake {
    pub const fn new() -> Self { Self { state: State::Unarmed } }

    /// Record a baseline only. A sample that is not a genuine `resolved == 0`
    /// reading under a bound identity changes nothing, and a detector that
    /// has already fired never re-arms. Returns whether a baseline is armed
    /// after this call.
    pub fn arm(&mut self, sample: Sample) -> bool {
        let Some(identity) = sample.identity else { return false; };
        if matches!(self.state, State::Committed(_)) || sample.resolved != 0 {
            return self.armed();
        }
        self.state = State::Armed(identity);
        true
    }

    /// Classify one fresh sample and advance the detector.
    pub fn observe(&mut self, sample: Sample) -> Observation {
        let Some(identity) = sample.identity else {
            self.state = State::Unarmed;
            return Observation::Unbound;
        };
        match self.state {
            State::Committed(armed) if armed == identity => Observation::Held,
            State::Armed(armed) if armed == identity => {
                if sample.resolved == 0 {
                    Observation::Waiting
                } else if sample.ack != 0 {
                    self.state = State::Committed(identity);
                    Observation::Committed
                } else {
                    Observation::Aborted
                }
            }
            // Nothing armed, or a baseline recorded under another service or
            // lobby generation: that baseline cannot qualify this sample.
            _ => {
                if sample.resolved == 0 {
                    self.state = State::Armed(identity);
                    Observation::Baseline
                } else {
                    self.state = State::Unarmed;
                    Observation::StaleHigh
                }
            }
        }
    }

    pub const fn armed(&self) -> bool { matches!(self.state, State::Armed(_)) }

    pub const fn fired(&self) -> bool { matches!(self.state, State::Committed(_)) }

    /// Explicit end of the owning session (refusal, timeout, expiry). The next
    /// session constructs its own detector; this one can no longer fire.
    pub fn end(&mut self) { self.state = State::Unarmed; }
}

/// What the waiting-room editor does with this callback's fresh observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interruption {
    /// A queued native event (match-start, remote Join or an unclassified
    /// notice) is pending: the existing priority, unchanged.
    Network(u64),
    /// The Local Wireless round committed while the editor owns the scene and
    /// no native event exists yet.
    Handshake,
}

/// `pending` is the bridge's existing queued-event result: `None` without a
/// service, `Some(0)` for "nothing pending" (which includes a resolved round),
/// otherwise the queued event. Only a Local Wireless session consults or
/// changes `handshake`; Quickplay keeps the old decision exactly.
pub fn interruption(
    local_wireless: bool, pending: Option<u64>, handshake: &mut Handshake, sample: Sample,
) -> Option<Interruption> {
    let observation = local_wireless.then(|| handshake.observe(sample));
    match pending {
        Some(event) if event != 0 => Some(Interruption::Network(event)),
        _ => (observation == Some(Observation::Committed)).then_some(Interruption::Handshake),
    }
}

/// Resume-room "no event" readiness. Quickplay is exactly the old expression;
/// a Local Wireless room whose round is already committed is not idle even
/// before its match-start event exists.
pub fn resume_no_event(local_wireless: bool, pending: Option<u64>, sample: Sample) -> bool {
    pending == Some(0) && !(local_wireless && sample.committed())
}
