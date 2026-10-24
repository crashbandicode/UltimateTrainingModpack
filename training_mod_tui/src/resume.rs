//! Owned, in-session UI suspension across native scene teardown.
//!
//! Adapters capture values, release their native UI, and reconstruct it later.
//! This layer neither saves feature settings nor confirms provisional edits.
//! The caller supplies a lobby-session identity, never a native object address.
//! There is one outstanding interaction; a snapshot survives a failed restore.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumeTicket {
    session: u64,
    serial: u64,
}

/// Facts from the current native callback, not facts cached before the match.
#[derive(Clone, Copy, Debug)]
pub struct ResumeReadiness {
    pub session: u64,
    pub previous_ui_released: bool,
    pub interruption_resolved: bool,
    pub practice_ready: bool,
    pub another_ui_active: bool,
}

impl ResumeReadiness {
    fn permits(self, ticket: ResumeTicket) -> bool {
        self.session == ticket.session
            && self.previous_ui_released
            && self.interruption_resolved
            && self.practice_ready
            && !self.another_ui_active
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Suspended,
    Restoring,
}

struct Entry<T> {
    ticket: ResumeTicket,
    snapshot: T,
    phase: Phase,
}

/// Stores value snapshots only. The feature adapter validates their contents.
/// No serialization is implied: this is scene continuation in the same process.
pub struct ResumeSlot<T> {
    serial: u64,
    entry: Option<Entry<T>>,
}

impl<T> Default for ResumeSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ResumeSlot<T> {
    pub const fn new() -> Self {
        Self {
            serial: 0,
            entry: None,
        }
    }

    /// Reject a competing snapshot rather than overwrite the unfinished work.
    /// On refusal, return ownership of the supplied value to its caller.
    pub fn suspend(&mut self, session: u64, snapshot: T) -> Result<ResumeTicket, T> {
        if self.entry.is_some() {
            return Err(snapshot);
        }
        let Some(serial) = self.serial.checked_add(1) else {
            return Err(snapshot);
        };
        self.serial = serial;
        let ticket = ResumeTicket { session, serial };
        self.entry = Some(Entry {
            ticket,
            snapshot,
            phase: Phase::Suspended,
        });
        Ok(ticket)
    }

    pub fn ticket(&self) -> Option<ResumeTicket> {
        self.entry.as_ref().map(|entry| entry.ticket)
    }

    /// Begin one attempt only when the adapter has observed an eligible return.
    /// The reference cannot outlive this slot borrow; copy owned values before
    /// invoking native code or crossing a thread boundary.
    pub fn begin_restore(
        &mut self,
        ticket: ResumeTicket,
        ready: ResumeReadiness,
    ) -> Option<&T> {
        let entry = self.entry.as_mut()?;
        if entry.ticket != ticket || entry.phase != Phase::Suspended || !ready.permits(ticket) {
            return None;
        }
        entry.phase = Phase::Restoring;
        Some(&entry.snapshot)
    }

    /// Refine an owned deferred value during its exact active reconstruction.
    /// The adapter must validate the refinement before writing it; this does not
    /// acknowledge restoration or change the ticket/readiness lifecycle.
    pub fn restoring_snapshot_mut(&mut self, ticket: ResumeTicket) -> Option<&mut T> {
        let entry = self.entry.as_mut()?;
        (entry.ticket == ticket && entry.phase == Phase::Restoring).then_some(&mut entry.snapshot)
    }

    /// Keep the checkpoint after a failed reconstruction. The adapter decides
    /// when there is new evidence to retry; this method does not start a loop.
    pub fn restore_failed(&mut self, ticket: ResumeTicket) -> bool {
        let Some(entry) = self.entry.as_mut() else {
            return false;
        };
        if entry.ticket != ticket || entry.phase != Phase::Restoring {
            return false;
        }
        entry.phase = Phase::Suspended;
        true
    }

    /// A later interruption preserves the unacknowledged checkpoint, whether
    /// reconstruction is still running or has already failed. Only its owner
    /// may suspend it again; this never replaces the original pending edits.
    pub fn interrupt_restore(&mut self, ticket: ResumeTicket) -> bool {
        let Some(entry) = self.entry.as_mut() else {
            return false;
        };
        if entry.ticket != ticket {
            return false;
        }
        entry.phase = Phase::Suspended;
        true
    }

    /// Acknowledge only after the requested UI, including pending edits, exists.
    /// This removes the checkpoint; it does not save or confirm those edits.
    pub fn restored(&mut self, ticket: ResumeTicket) -> bool {
        if !self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.ticket == ticket && entry.phase == Phase::Restoring)
        {
            return false;
        }
        self.entry = None;
        true
    }

    /// Explicit cancellation or leaving the owning lobby discards the snapshot.
    /// A stale callback cannot discard a newer interaction.
    pub fn discard(&mut self, ticket: ResumeTicket) -> bool {
        if !self.entry.as_ref().is_some_and(|entry| entry.ticket == ticket) {
            return false;
        }
        self.entry = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{ResumeReadiness, ResumeSlot};

    #[derive(Debug, PartialEq, Eq)]
    struct MovesScreen {
        confirmed: [u8; 4],
        pending: [u8; 4],
        focused_row: u8,
    }

    fn ready(session: u64) -> ResumeReadiness {
        ResumeReadiness {
            session,
            previous_ui_released: true,
            interruption_resolved: true,
            practice_ready: true,
            another_ui_active: false,
        }
    }

    #[test]
    fn pending_moves_survive_match_without_becoming_confirmed() {
        let mut slot = ResumeSlot::new();
        let ticket = slot
            .suspend(
                7,
                MovesScreen {
                    confirmed: [2, 1, 1, 1],
                    pending: [2, 2, 1, 1],
                    focused_row: 1,
                },
            )
            .unwrap();
        let blocked = [
            ResumeReadiness { previous_ui_released: false, ..ready(7) },
            ResumeReadiness { interruption_resolved: false, ..ready(7) },
            ResumeReadiness { practice_ready: false, ..ready(7) },
            ResumeReadiness { another_ui_active: true, ..ready(7) },
            ready(8),
        ];
        for observation in blocked {
            assert!(slot.begin_restore(ticket, observation).is_none());
        }
        let screen = slot.begin_restore(ticket, ready(7)).unwrap();
        assert_eq!(screen.confirmed, [2, 1, 1, 1]);
        assert_eq!(screen.pending, [2, 2, 1, 1]);
        assert_eq!(screen.focused_row, 1);
        assert!(slot.begin_restore(ticket, ready(7)).is_none());
        assert!(slot.restored(ticket));
        assert!(slot.ticket().is_none());
    }

    #[test]
    fn failed_keyboard_restore_retains_unconfirmed_text_and_caret() {
        let mut slot = ResumeSlot::new();
        // The adapter, not this generic layer, defines and validates the payload.
        let ticket = slot.suspend(9, ("Original", "Resume", 3_usize)).unwrap();
        assert!(!slot.restored(ticket));
        assert_eq!(slot.begin_restore(ticket, ready(9)), Some(&("Original", "Resume", 3)));
        assert!(slot.restore_failed(ticket));
        assert_eq!(slot.begin_restore(ticket, ready(9)), Some(&("Original", "Resume", 3)));
        assert!(slot.restored(ticket));
    }

    #[test]
    fn competing_request_and_stale_completion_cannot_destroy_current_work() {
        let mut slot = ResumeSlot::new();
        let old = slot.suspend(4, "first").unwrap();
        assert_eq!(slot.suspend(4, "second"), Err("second"));
        assert!(slot.discard(old));
        let current = slot.suspend(4, "second").unwrap();
        assert!(!slot.discard(old));
        assert!(slot.begin_restore(old, ready(4)).is_none());
        assert_eq!(slot.begin_restore(current, ready(4)), Some(&"second"));
        assert!(!slot.restore_failed(old));
        assert!(!slot.restored(old));
        assert!(slot.restored(current));
    }

    #[test]
    fn explicit_cancel_does_not_leave_work_to_resume() {
        let mut slot = ResumeSlot::new();
        let ticket = slot.suspend(2, "unsaved").unwrap();
        assert!(slot.discard(ticket));
        assert!(slot.begin_restore(ticket, ready(2)).is_none());
        assert!(!slot.restore_failed(ticket));
        assert!(slot.ticket().is_none());
    }

    #[test]
    fn failed_restore_then_second_interruption_keeps_original_pending_screen() {
        let mut slot = ResumeSlot::new();
        let ticket = slot.suspend(12, ([1, 1, 0, 0], 2_u8)).unwrap();
        assert_eq!(slot.begin_restore(ticket, ready(12)), Some(&([1, 1, 0, 0], 2)));
        assert!(slot.restore_failed(ticket));
        // The adapter must keep this ticket instead of offering a second
        // snapshot from the partly reconstructed UI to the occupied slot.
        assert!(slot.interrupt_restore(ticket));
        assert!(slot.begin_restore(ticket, ready(13)).is_none());
        assert_eq!(slot.begin_restore(ticket, ready(12)), Some(&([1, 1, 0, 0], 2)));
        assert!(slot.interrupt_restore(ticket));
        assert_eq!(slot.begin_restore(ticket, ready(12)), Some(&([1, 1, 0, 0], 2)));
        assert!(slot.restored(ticket));
        let next = slot.suspend(12, ([0, 0, 0, 0], 0)).unwrap();
        assert!(!slot.interrupt_restore(ticket));
        assert_eq!(slot.begin_restore(next, ready(12)), Some(&([0, 0, 0, 0], 0)));
    }

    #[test]
    fn deferred_value_refinement_survives_reinterruption_under_exact_ticket() {
        let mut slot = ResumeSlot::new();
        let old = slot.suspend(12, None::<[u8; 3]>).unwrap();
        assert!(slot.discard(old));
        let ticket = slot.suspend(12, None::<[u8; 3]>).unwrap();
        assert!(slot.restoring_snapshot_mut(ticket).is_none());
        assert_eq!(slot.begin_restore(ticket, ready(12)), Some(&None));
        assert!(slot.restoring_snapshot_mut(old).is_none());
        *slot.restoring_snapshot_mut(ticket).unwrap() = Some([7, 8, 9]);
        assert!(slot.interrupt_restore(ticket));
        assert!(slot.restoring_snapshot_mut(ticket).is_none());
        assert_eq!(slot.begin_restore(ticket, ready(12)), Some(&Some([7, 8, 9])));
        assert!(slot.restore_failed(ticket));
        assert!(slot.restoring_snapshot_mut(ticket).is_none());
        assert_eq!(slot.begin_restore(ticket, ready(12)), Some(&Some([7, 8, 9])));
        assert!(slot.restored(ticket));
        assert!(slot.restoring_snapshot_mut(ticket).is_none());
    }
}
