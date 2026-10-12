//! Bounded observations associated with native replay records, not the latest match.
//! The adapter supplies verified record/reset events and native serializer counts.
//! This module neither recognizes game modes nor decides whether a save succeeded.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::{LiveFrame, LiveSession, LiveTrack, MAX_FRAMES};

/// An incarnation changes even when the game resets/reuses the same native index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeRecordKey {
    pub generation: u64,
    pub index: u32,
}

/// Includes the active record. Older evicted records are unavailable, never rebound
/// to the newest capture. At most 4 * 8 * MAX_FRAMES samples are retained internally.
pub const MAX_RETAINED_CAPTURES: usize = 4;
const ENTRIES: usize = 8;

struct LiveRecord {
    session: LiveSession,
    last_tick: [Option<u32>; ENTRIES],
    sealed: [bool; ENTRIES],
}

struct Record {
    key: NativeRecordKey,
    live: Option<LiveRecord>,
    saved: Option<Arc<LiveSession>>,
}

pub struct LiveCapturePool {
    generation: u64,
    active: Option<NativeRecordKey>,
    records: VecDeque<Record>,
}

impl Default for LiveCapturePool {
    fn default() -> Self { Self::new() }
}

impl LiveCapturePool {
    pub const fn new() -> Self {
        Self { generation: 0, active: None, records: VecDeque::new() }
    }

    /// Drop only pool ownership. A save already bound to an Arc remains valid.
    pub fn clear(&mut self) {
        self.active = None;
        self.records.clear();
    }

    pub fn start_record(&mut self, index: u32) -> NativeRecordKey {
        self.finish_record();
        self.records.retain(|record| record.key.index != index);
        while self.records.len() >= MAX_RETAINED_CAPTURES {
            self.records.pop_front();
        }
        self.generation += 1;
        let key = NativeRecordKey { generation: self.generation, index };
        self.records.push_back(Record {
            key,
            live: Some(LiveRecord {
                session: LiveSession { tracks: Vec::with_capacity(ENTRIES),
                    truncated: false, first_counter: None },
                last_tick: [None; ENTRIES],
                sealed: [false; ENTRIES],
            }),
            saved: None,
        });
        self.active = Some(key);
        key
    }

    pub fn active_key(&self) -> Option<NativeRecordKey> { self.active }

    /// A native prefix reset replaces a slot without changing its numeric index.
    /// Previously bound immutable snapshots remain owned by their save operation.
    pub fn invalidate_record(&mut self, index: u32) {
        if self.active.is_some_and(|key| key.index == index) { self.active = None; }
        self.records.retain(|record| record.key.index != index);
    }

    pub fn key_for_index(&self, index: u32) -> Option<NativeRecordKey> {
        self.records.iter().find(|record| record.key.index == index).map(|record| record.key)
    }

    /// Skip redundant native getters for a repeated callback. A different count
    /// still reaches push(), which detects and seals gaps/backwards movement.
    pub fn wants_sample(&self, key: NativeRecordKey, entry: i32, tick: u32) -> bool {
        if self.active != Some(key) || !(0..ENTRIES as i32).contains(&entry) { return false; }
        self.records.back().and_then(|record| record.live.as_ref()).is_some_and(|live| {
            !live.sealed[entry as usize] && live.last_tick[entry as usize] != Some(tick)
        })
    }

    /// Reject one fighter's discontinuous sample without ending other tracks.
    /// A stale key cannot seal an entry in a newer native-record incarnation.
    pub fn seal_entry(&mut self, key: NativeRecordKey, entry: i32) {
        if self.active != Some(key) || !(0..ENTRIES as i32).contains(&entry) { return; }
        if let Some(live) = self.records.back_mut().and_then(|record| record.live.as_mut()) {
            live.sealed[entry as usize] = true;
            live.session.truncated = true;
        }
    }

    pub fn push(&mut self, key: NativeRecordKey, entry: i32, tick: u32,
                sample: LiveFrame) -> bool {
        if self.active != Some(key) || !(0..ENTRIES as i32).contains(&entry) { return false; }
        let Some(live) = self.records.back_mut().and_then(|record| record.live.as_mut()) else {
            return false;
        };
        let slot = entry as usize;
        if live.sealed[slot] || live.last_tick[slot] == Some(tick) { return false; }
        let gap = match live.last_tick[slot] {
            Some(last) => last.checked_add(1) != Some(tick),
            // The editor carries crop indexes across fighters. A late-starting
            // track must not silently shift the selected match time.
            None => live.session.first_counter.is_some_and(|first| first != tick),
        };
        if gap || !sample.is_valid() {
            live.sealed[slot] = true;
            live.session.truncated = true;
            return false;
        }
        let track_index = match live.session.tracks.iter().position(|track| track.entry == entry) {
            Some(index) => index,
            None => {
                let mut frames = Vec::new();
                // One bounded reservation when this fighter first appears. No
                // growth/allocation on subsequent simulation samples.
                if frames.try_reserve_exact(MAX_FRAMES).is_err() {
                    live.sealed[slot] = true;
                    live.session.truncated = true;
                    return false;
                }
                live.session.tracks.push(LiveTrack { entry, frames });
                live.session.tracks.len() - 1
            }
        };
        let track = &mut live.session.tracks[track_index];
        if track.frames.len() == MAX_FRAMES {
            live.sealed[slot] = true;
            live.session.truncated = true;
            return false;
        }
        track.frames.push(sample);
        live.session.first_counter.get_or_insert(tick);
        live.last_tick[slot] = Some(tick);
        true
    }

    pub fn finish_record(&mut self) {
        let Some(key) = self.active.take() else { return };
        if let Some(record) = self.records.iter_mut().find(|record| record.key == key) {
            record.saved = record.live.take().filter(|live| !live.session.tracks.is_empty())
                .map(|mut live| {
                    for track in &mut live.session.tracks { track.frames.shrink_to_fit(); }
                    Arc::new(live.session)
                });
        }
        self.records.retain(|record| record.live.is_some() || record.saved.is_some());
    }

    /// Preserve a valid prefix when the native record itself becomes discontinuous.
    pub fn finish_partial(&mut self) {
        if self.active.is_some() {
            if let Some(live) = self.records.back_mut().and_then(|record| record.live.as_mut()) {
                live.session.truncated = true;
            }
        }
        self.finish_record();
    }

    /// Called at the native indexed-copy boundary, separately from save success.
    /// The caller bounds outstanding saves; clones share immutable frame storage.
    pub fn snapshot(&mut self, key: NativeRecordKey) -> Option<Arc<LiveSession>> {
        if self.active == Some(key) { self.finish_record(); }
        self.records.iter().find(|record| record.key == key)?.saved.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LiveInput, StartState};

    fn sample(buttons: u32) -> LiveFrame {
        LiveFrame { input: LiveInput { buttons, ..LiveInput::default() },
            start: StartState::neutral(8, 0, 1.0), x: 0.0, y: 0.0,
            position_z: 0.0, speed_z: 0.0, body: 1 }
    }

    #[test]
    fn successive_records_bind_out_of_order_and_survive_pool_reset() {
        let mut pool = LiveCapturePool::new();
        let first = pool.start_record(0);
        assert!(pool.push(first, 1, 200, sample(11)));
        let second = pool.start_record(1);
        let mut precise = sample(22);
        precise.input.axes[0] = 0.12345679;
        assert!(pool.push(second, 1, 400, precise));
        let later = pool.snapshot(second).unwrap();
        let earlier = pool.snapshot(first).unwrap();
        assert_eq!(later.tracks[0].frames[0].input.buttons, 22);
        assert_eq!(later.tracks[0].frames[0].input.axes[0].to_bits(), precise.input.axes[0].to_bits());
        assert_eq!(later.first_counter, Some(400));
        assert_eq!(earlier.tracks[0].frames[0].input.buttons, 11);
        assert!(Arc::ptr_eq(&earlier, &pool.snapshot(first).unwrap()));
        pool.clear();
        assert!(pool.key_for_index(0).is_none());
        assert!(pool.snapshot(first).is_none());
        assert_eq!(earlier.tracks[0].frames[0].input.buttons, 11);
    }

    #[test]
    fn reused_slot_rejects_stale_sampler_and_save_keys() {
        let mut pool = LiveCapturePool::new();
        let old = pool.start_record(3);
        assert!(pool.push(old, 0, 5, sample(1)));
        let new = pool.start_record(3);
        assert_ne!(old, new);
        assert!(!pool.push(old, 0, 6, sample(99)));
        assert!(pool.snapshot(old).is_none());
        assert_eq!(pool.key_for_index(3), Some(new));
        assert!(pool.push(new, 0, 5, sample(2)));
        pool.clear();
        let after_clear = pool.start_record(3);
        assert_ne!(new, after_clear);
        assert!(!pool.push(new, 0, 6, sample(99)));
    }

    #[test]
    fn duplicates_do_not_advance_and_gaps_never_compress_time() {
        for bad_tick in [9, 6] {
            let mut pool = LiveCapturePool::new();
            let key = pool.start_record(0);
            assert!(pool.push(key, 0, 7, sample(1)));
            assert!(!pool.wants_sample(key, 0, 7));
            assert!(!pool.push(key, 0, 7, sample(99)));
            assert!(pool.wants_sample(key, 0, bad_tick));
            assert!(!pool.push(key, 0, bad_tick, sample(2)));
            assert!(!pool.push(key, 0, 8, sample(3)));
            assert!(pool.push(key, 1, 7, sample(4)));
            assert!(pool.push(key, 1, 8, sample(5)));
            let saved = pool.snapshot(key).unwrap();
            assert!(saved.truncated);
            assert_eq!(saved.tracks[0].frames.len(), 1);
            assert_eq!(saved.tracks[1].frames.len(), 2);
        }
    }

    #[test]
    fn invalid_samples_and_entries_cannot_poison_an_archive() {
        let mut pool = LiveCapturePool::new();
        let key = pool.start_record(0);
        assert!(!pool.push(key, -1, 0, sample(1)));
        assert!(!pool.push(key, 8, 0, sample(1)));
        let mut invalid = sample(1);
        invalid.y = f32::NAN;
        assert!(!pool.push(key, 0, 0, invalid));
        assert!(!pool.push(key, 0, 1, sample(1)));
        assert!(pool.snapshot(key).is_none());
        let key = pool.start_record(1);
        assert!(pool.push(key, 1, u32::MAX, sample(2)));
        assert!(!pool.push(key, 1, 0, sample(3)));
        assert!(pool.snapshot(key).unwrap().truncated);
    }

    #[test]
    fn frame_limit_stops_one_track_without_stopping_other_fighters() {
        let mut pool = LiveCapturePool::new();
        let key = pool.start_record(0);
        assert!(pool.push(key, 1, 0, sample(41)));
        for tick in 0..MAX_FRAMES as u32 {
            assert!(pool.push(key, 0, tick, sample(tick)));
        }
        assert!(!pool.push(key, 0, MAX_FRAMES as u32, sample(99)));
        assert!(pool.push(key, 1, 1, sample(42)));
        let saved = pool.snapshot(key).unwrap();
        assert!(saved.truncated);
        assert_eq!(saved.tracks[1].frames.len(), MAX_FRAMES);
        assert_eq!(saved.tracks[0].frames.len(), 2);
    }

    #[test]
    fn retention_eviction_is_unavailable_instead_of_latest_match() {
        let mut pool = LiveCapturePool::new();
        let first = pool.start_record(0);
        assert!(pool.push(first, 0, 0, sample(0)));
        for index in 1..=MAX_RETAINED_CAPTURES as u32 {
            let key = pool.start_record(index);
            assert!(pool.push(key, 0, 0, sample(index)));
        }
        assert_eq!(pool.records.len(), MAX_RETAINED_CAPTURES);
        assert!(pool.key_for_index(0).is_none());
        assert!(pool.snapshot(first).is_none());
        let current = pool.active_key().unwrap();
        assert_eq!(pool.snapshot(current).unwrap().tracks[0].frames[0].input.buttons,
                   MAX_RETAINED_CAPTURES as u32);
    }

    #[test]
    fn native_discontinuity_keeps_only_the_valid_partial_prefix() {
        let mut pool = LiveCapturePool::new();
        let key = pool.start_record(0);
        assert!(pool.push(key, 1, 8, sample(17)));
        pool.finish_partial();
        assert!(!pool.push(key, 1, 0, sample(99)));
        let saved = pool.snapshot(key).unwrap();
        assert!(saved.truncated);
        assert_eq!(saved.tracks[0].frames.len(), 1);
        assert_eq!(saved.tracks[0].frames.capacity(), 1);
        assert_eq!(saved.tracks[0].frames[0].input.buttons, 17);
        pool.finish_partial();
        assert!(Arc::ptr_eq(&saved, &pool.snapshot(key).unwrap()));
    }

    #[test]
    fn late_fighter_cannot_shift_shared_crop_time() {
        let mut pool = LiveCapturePool::new();
        let key = pool.start_record(0);
        assert!(pool.push(key, 0, 20, sample(1)));
        assert!(!pool.push(key, 1, 21, sample(2)));
        assert!(!pool.push(key, 1, 20, sample(3)));
        assert!(pool.push(key, 2, 20, sample(4)));
        let saved = pool.snapshot(key).unwrap();
        assert!(saved.truncated);
        assert_eq!(saved.tracks.iter().map(|track| track.entry).collect::<Vec<_>>(), vec![0, 2]);
    }

    #[test]
    fn rejected_callback_seals_only_its_entry_and_never_a_new_record() {
        let mut pool = LiveCapturePool::new();
        let key = pool.start_record(0);
        assert!(pool.push(key, 0, 10, sample(1)));
        assert!(pool.push(key, 1, 10, sample(2)));
        pool.seal_entry(key, 0);
        assert_eq!(pool.active_key(), Some(key));
        assert!(!pool.wants_sample(key, 0, 11));
        assert!(!pool.push(key, 0, 11, sample(3)));
        assert!(pool.push(key, 1, 11, sample(4)));
        let saved = pool.snapshot(key).unwrap();
        assert!(saved.truncated);
        assert_eq!(saved.tracks[0].frames.len(), 1);
        assert_eq!(saved.tracks[1].frames.len(), 2);
        let next = pool.start_record(0);
        pool.seal_entry(key, 1);
        pool.seal_entry(next, -1);
        pool.seal_entry(next, 8);
        assert!(pool.push(next, 1, 10, sample(5)));
        assert!(!pool.snapshot(next).unwrap().truncated);
    }

    #[test]
    fn native_prefix_replacement_invalidates_only_that_slot() {
        let mut pool = LiveCapturePool::new();
        let older = pool.start_record(0);
        assert!(pool.push(older, 0, 0, sample(1)));
        let replaced = pool.start_record(1);
        assert!(pool.push(replaced, 0, 0, sample(2)));
        let pending_save = pool.snapshot(replaced).unwrap();
        pool.invalidate_record(1);
        assert!(pool.key_for_index(1).is_none());
        assert!(pool.snapshot(replaced).is_none());
        assert!(!pool.wants_sample(replaced, 0, 1));
        assert_eq!(pool.snapshot(older).unwrap().tracks[0].frames[0].input.buttons, 1);
        assert_eq!(pending_save.tracks[0].frames[0].input.buttons, 2);
        let fresh = pool.start_record(1);
        assert_ne!(fresh, replaced);
        pool.invalidate_record(1);
        assert!(pool.active_key().is_none());
        assert!(!pool.push(fresh, 0, 0, sample(3)));
    }

    #[test]
    fn eight_aligned_fighters_survive_archive_reopen_and_duplicate_save() {
        let temp = crate::test_util::TempDir::new();
        let mut pool = LiveCapturePool::new();
        let key = pool.start_record(0);
        for tick in 10..12 {
            for entry in 0..8 {
                let mut frame = sample(tick * 8 + entry as u32);
                frame.body = entry as u32;
                if entry == 3 { frame.input.axes[0] = 0.12345679; }
                assert!(pool.push(key, entry, tick, frame));
            }
        }
        let session = pool.snapshot(key).unwrap();
        let archive = crate::SharedCaptureArchive::new(temp.path());
        let first = archive.save_with_outcome("native", "Eight fighters", "13.0.5", &session).unwrap();
        assert!(first.created);
        let duplicate = archive.save_with_outcome("native", "Eight fighters", "13.0.5", &session).unwrap();
        assert!(!duplicate.created);
        assert_eq!(first.id, duplicate.id);
        pool.clear();
        let restored = crate::CaptureArchive::new(temp.path()).load(&first.id).unwrap();
        assert_eq!(restored.session.tracks.len(), 8);
        for (before, after) in session.tracks.iter().zip(&restored.session.tracks) {
            assert_eq!(before.frames.capacity(), before.frames.len());
            assert_eq!(before.entry, after.entry);
            assert_eq!(after.quantized_analog, before.entry == 3);
            assert_eq!(before.frames.iter().map(|frame| frame.input.project().0).collect::<Vec<_>>(),
                       after.frames.iter().map(|frame| frame.input).collect::<Vec<_>>());
        }
    }
}
