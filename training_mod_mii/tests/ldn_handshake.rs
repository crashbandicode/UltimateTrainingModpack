//! Original probes for the Local Wireless round-handshake policy, through its
//! real entry points. Oracle: run124's three guest samples while the editor
//! owned the scene (ack 0,1,1 / resolved 0,1,1, deque unchanged) and the
//! audited native consumer predicate. Addresses are identity tokens only.
use training_mod_mii::ldn_handshake::{
    interruption, resume_no_event, Handshake, Identity, Interruption, Observation, Sample,
};

const SERVICE: usize = 0x0011_dc81_0000;
const OTHER_SERVICE: usize = 0x0011_dc82_0000;
const GENERATION: u64 = 7;

fn sample(service: usize, generation: u64, ack: u32, resolved: u8) -> Sample {
    Sample { identity: Some(Identity { service, generation }), ack, resolved }
}

fn guest(ack: u32, resolved: u8) -> Sample { sample(SERVICE, GENERATION, ack, resolved) }

fn unbound(ack: u32, resolved: u8) -> Sample { Sample { identity: None, ack, resolved } }

#[test]
fn run124_edge_fires_once_from_a_recorded_zero_baseline() {
    let mut handshake = Handshake::new();
    assert_eq!(handshake, Handshake::default());
    assert!(!handshake.armed() && !handshake.fired());
    // Before host Start: both zero.
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    assert!(handshake.armed());
    // The guest acknowledges first; the round is still unresolved.
    assert_eq!(handshake.observe(guest(1, 0)), Observation::Waiting);
    assert!(!guest(1, 0).committed() && !guest(1, 0).aborted());
    // +1 s: resolved with the host's bit in the mask.
    assert!(guest(1, 1).committed());
    assert_eq!(handshake.observe(guest(1, 1)), Observation::Committed);
    assert!(handshake.fired() && !handshake.armed());
    // +10 s and every later frame of this session: held, never a second edge.
    assert_eq!(handshake.observe(guest(1, 1)), Observation::Held);
    assert_eq!(handshake.observe(guest(1, 1)), Observation::Held);
    // A native clear later in the same session does not re-arm it either.
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Held);
    assert!(handshake.fired());
}

#[test]
fn resolved_high_at_entry_cannot_fire_until_a_genuine_zero_is_recorded() {
    let mut handshake = Handshake::new();
    for _ in 0..3 {
        assert_eq!(handshake.observe(guest(1, 1)), Observation::StaleHigh);
        assert!(!handshake.fired() && !handshake.armed());
    }
    // A round the service resets afterwards is a fresh baseline for this session.
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(handshake.observe(guest(1, 1)), Observation::Committed);
}

#[test]
fn abort_is_never_begin_and_does_not_disarm_the_session() {
    let mut handshake = Handshake::new();
    // Resolved with an empty mask before any baseline: stale, not an abort edge.
    assert_eq!(handshake.observe(guest(0, 1)), Observation::StaleHigh);
    assert!(guest(0, 1).aborted() && !guest(0, 1).committed());
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(handshake.observe(guest(0, 1)), Observation::Aborted);
    assert_eq!(handshake.observe(guest(0, 1)), Observation::Aborted);
    assert!(handshake.armed() && !handshake.fired());
    // The round resets and resolves again with an acknowledgement.
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Waiting);
    assert_eq!(handshake.observe(guest(1, 1)), Observation::Committed);
    // A sequential read that saw the resolved byte before the mask must not
    // permanently disarm a session whose next read shows the committed round.
    let mut racing = Handshake::new();
    assert_eq!(racing.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(racing.observe(guest(0, 1)), Observation::Aborted);
    assert_eq!(racing.observe(guest(1, 1)), Observation::Committed);
}

#[test]
fn another_service_or_lobby_generation_drops_the_baseline() {
    let mut handshake = Handshake::new();
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    // Same generation, different service object: the recorded zero does not qualify it.
    assert_eq!(handshake.observe(sample(OTHER_SERVICE, GENERATION, 1, 1)), Observation::StaleHigh);
    assert!(!handshake.fired() && !handshake.armed());
    assert_eq!(handshake.observe(sample(OTHER_SERVICE, GENERATION, 0, 0)), Observation::Baseline);
    // Back on the original service with set flags: still not an edge of the new baseline.
    assert_eq!(handshake.observe(guest(1, 1)), Observation::StaleHigh);
    // Same service, replaced lobby generation.
    assert_eq!(handshake.observe(sample(SERVICE, GENERATION + 1, 0, 0)), Observation::Baseline);
    assert_eq!(handshake.observe(guest(1, 1)), Observation::StaleHigh);
    assert!(!handshake.fired());
    // A service chain that cannot be rebound disarms; the following high read is stale.
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(handshake.observe(unbound(1, 1)), Observation::Unbound);
    assert!(!handshake.armed());
    assert_eq!(handshake.observe(guest(1, 1)), Observation::StaleHigh);
    // After a fired edge, a different identity is evaluated as unarmed.
    let mut fired = Handshake::new();
    assert_eq!(fired.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(fired.observe(guest(1, 1)), Observation::Committed);
    assert_eq!(fired.observe(sample(OTHER_SERVICE, GENERATION, 1, 1)), Observation::StaleHigh);
    assert!(!fired.fired());
}

#[test]
fn arm_records_only_a_genuine_zero_baseline_and_never_fires() {
    let mut handshake = Handshake::new();
    assert!(!handshake.arm(guest(1, 1)));
    assert!(!handshake.arm(guest(0, 1)));
    assert!(!handshake.arm(unbound(0, 0)));
    assert!(!handshake.armed());
    // An acknowledged but unresolved round is still a resolved-zero baseline.
    assert!(handshake.arm(guest(1, 0)));
    assert!(handshake.armed() && !handshake.fired());
    // Arming again with a high reading keeps the existing baseline.
    assert!(handshake.arm(guest(1, 1)));
    assert!(handshake.armed());
    assert_eq!(handshake.observe(guest(1, 1)), Observation::Committed);
    // A fired detector never re-arms.
    assert!(!handshake.arm(guest(0, 0)));
    assert!(handshake.fired());
}

#[test]
fn queued_match_and_remote_join_keep_priority_over_the_handshake() {
    let mut handshake = Handshake::new();
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    // A queued match-start (kind 6) already pending wins even on the committed frame.
    assert_eq!(interruption(true, Some(6), &mut handshake, guest(1, 1)), Some(Interruption::Network(6)));
    // A remote Join notice with its sender slot likewise keeps the existing path.
    let remote_join = 1 | (3_u64 << 48);
    let mut joined = Handshake::new();
    assert_eq!(joined.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(interruption(true, Some(remote_join), &mut joined, guest(0, 0)),
        Some(Interruption::Network(remote_join)));
    // Nothing queued and the round unresolved: the editor keeps running.
    let mut quiet = Handshake::new();
    assert_eq!(interruption(true, Some(0), &mut quiet, guest(0, 0)), None);
    assert_eq!(interruption(true, Some(0), &mut quiet, guest(1, 0)), None);
    // Nothing queued and the round committed: the typed handshake interruption, once.
    assert_eq!(interruption(true, Some(0), &mut quiet, guest(1, 1)), Some(Interruption::Handshake));
    assert_eq!(interruption(true, Some(0), &mut quiet, guest(1, 1)), None);
    // An abort with nothing queued never interrupts.
    let mut aborted = Handshake::new();
    assert_eq!(interruption(true, Some(0), &mut aborted, guest(0, 0)), None);
    assert_eq!(interruption(true, Some(0), &mut aborted, guest(0, 1)), None);
    // No service at all: no decision and no baseline.
    let mut missing = Handshake::new();
    assert_eq!(interruption(true, None, &mut missing, unbound(1, 1)), None);
    assert!(!missing.armed());
}

#[test]
fn quickplay_never_consults_the_detector_or_changes_its_resume_gate() {
    let mut handshake = Handshake::new();
    assert_eq!(interruption(false, Some(0), &mut handshake, guest(0, 0)), None);
    assert_eq!(interruption(false, Some(0), &mut handshake, guest(1, 1)), None);
    assert_eq!(handshake, Handshake::new());
    assert_eq!(interruption(false, Some(6), &mut handshake, guest(1, 1)), Some(Interruption::Network(6)));
    assert_eq!(interruption(false, None, &mut handshake, Sample::UNBOUND), None);
    assert_eq!(handshake, Handshake::new());
    // Quickplay's "no event" is exactly the queued-event result.
    assert!(resume_no_event(false, Some(0), guest(1, 1)));
    assert!(!resume_no_event(false, Some(6), guest(0, 0)));
    assert!(!resume_no_event(false, None, guest(0, 0)));
}

#[test]
fn committed_local_wireless_round_is_not_an_idle_resume_room() {
    assert!(resume_no_event(true, Some(0), guest(0, 0)));
    assert!(resume_no_event(true, Some(0), guest(1, 0)));
    // An aborted round is not a committed one; the old readiness stands.
    assert!(resume_no_event(true, Some(0), guest(0, 1)));
    assert!(!resume_no_event(true, Some(0), guest(1, 1)));
    assert!(!resume_no_event(true, Some(0), sample(OTHER_SERVICE, GENERATION + 1, 1, 1)));
    assert!(!resume_no_event(true, Some(6), guest(0, 0)));
    assert!(!resume_no_event(true, None, Sample::UNBOUND));
    assert!(resume_no_event(true, Some(0), Sample::UNBOUND));
}

#[test]
fn ending_a_session_resets_the_detector_and_a_new_session_fires_on_its_own_edge() {
    let mut handshake = Handshake::new();
    assert_eq!(handshake.observe(guest(0, 0)), Observation::Baseline);
    handshake.end();
    assert!(!handshake.armed());
    assert_eq!(handshake.observe(guest(1, 1)), Observation::StaleHigh);
    // A fired detector that is ended cannot report Held or fire again.
    let mut fired = Handshake::new();
    assert_eq!(fired.observe(guest(0, 0)), Observation::Baseline);
    assert_eq!(fired.observe(guest(1, 1)), Observation::Committed);
    fired.end();
    assert!(!fired.fired());
    assert_eq!(fired.observe(guest(1, 1)), Observation::StaleHigh);
    // The next editor session, under the rebuilt lobby, starts from its own zero.
    let mut next = Handshake::new();
    assert_eq!(next.observe(sample(SERVICE, GENERATION + 1, 1, 1)), Observation::StaleHigh);
    assert_eq!(next.observe(sample(SERVICE, GENERATION + 1, 0, 0)), Observation::Baseline);
    assert_eq!(interruption(true, Some(0), &mut next, sample(SERVICE, GENERATION + 1, 1, 1)),
        Some(Interruption::Handshake));
}
