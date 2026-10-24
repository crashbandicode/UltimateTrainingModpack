//! Private continuous recorder primitives. No target SDK or allocator dependencies.
//! Producers make one try-lock attempt and never overwrite unread records. All IO
//! belongs to the single consumer. See change-trace98/RECORDER_FORMAT.md for v2.

use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const QUEUE_CAPACITY: usize = 16_384;
pub const RECORD_BYTES: usize = 80;
pub const CHUNK_BYTES: usize = 65_536;
pub const CHUNK_HEADER: usize = 256;
pub const ROWS_PER_CHUNK: usize = (CHUNK_BYTES - CHUNK_HEADER - 16) / RECORD_BYTES;
// Largest whole-chunk file strictly below 4 GiB, including all metadata. FAT32
// permits at most 2^32 - 1 bytes; a 4 GiB file itself would not fit.
pub const DISK_CAP: u64 = 4 * 1024 * 1024 * 1024 - CHUNK_BYTES as u64;
pub const SLOT_COUNT: u64 = DISK_CAP / CHUNK_BYTES as u64;
pub const FLUSH_MS: u64 = 100;
pub const SESSION: u64 = 1;
pub const EVENTS: u64 = 2;
pub const CAPPED: u64 = 3;
pub const WRITE_FAILED: u64 = 4;
pub const PAUSE_BEGIN: u64 = 5;
pub const PAUSE_END: u64 = 6;
pub type Record = [u64; 10];

// Stable control-v1 status numbers. Unavailable (0) is supplied by callers when
// the feature/symbol is absent. Starting includes boot's pre-init capture.
pub const STARTING: u64 = 1;
pub const RECORDING: u64 = 2;
pub const STOPPING: u64 = 3;
pub const PAUSED: u64 = 4;
pub const STATUS_CAPPED: u64 = 5;
pub const STATUS_ERROR: u64 = 6;
const STATE_MASK: u64 = 7;
const ACCEPTING: u64 = 8;
const GENERATION: u64 = 16;
const GENERATION_MASK: u64 = !(GENERATION - 1);

struct Inner<const N: usize> {
    records: [Record; N],
    accepted: u64,
    drained: u64,
}

pub struct Queue<const N: usize = QUEUE_CAPACITY> {
    locked: AtomicBool,
    // State + admission + generation share one modification order. Only the
    // writer reopens admission, after sealing the resume baseline. A producer
    // delayed across a pause/resume must reject its old generation ticket.
    control: AtomicU64,
    busy: AtomicU64,
    full: AtomicU64,
    persisted_bytes: AtomicU64,
    last_error: AtomicU64,
    inner: UnsafeCell<Inner<N>>,
}

// All access to inner is covered by Guard. Reentrant producers simply drop.
unsafe impl<const N: usize> Sync for Queue<N> {}

struct Guard<'a, const N: usize>(&'a Queue<N>);
impl<const N: usize> Guard<'_, N> {
    fn inner(&mut self) -> &mut Inner<N> {
        // SAFETY: this guard exclusively owns the acquired lock until Drop.
        unsafe { &mut *self.0.inner.get() }
    }
}
impl<const N: usize> Drop for Guard<'_, N> {
    fn drop(&mut self) {
        self.0.locked.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Stats {
    pub accepted: u64,
    pub drained: u64,
    pub busy: u64,
    pub full: u64,
}
impl Stats {
    pub fn queued(self) -> u64 {
        self.accepted - self.drained
    }
}

impl<const N: usize> Queue<N> {
    pub const fn new() -> Self {
        assert!(N > 0);
        Self {
            locked: AtomicBool::new(false),
            control: AtomicU64::new(STARTING | ACCEPTING),
            busy: AtomicU64::new(0),
            full: AtomicU64::new(0),
            persisted_bytes: AtomicU64::new(0),
            last_error: AtomicU64::new(0),
            inner: UnsafeCell::new(Inner {
                records: [[0; 10]; N],
                accepted: 0,
                drained: 0,
            }),
        }
    }

    fn lock(&self) -> Option<Guard<'_, N>> {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Guard(self))
    }

    #[inline]
    #[allow(dead_code)] // Explicit-timestamp entry is also used by the host harness.
    pub fn record(&self, tick: u64, thread: u64, code: u64, args: [u64; 5]) -> bool {
        let Some(ticket) = self.admission_ticket() else {
            return false;
        };
        self.record_ticket(ticket, tick, thread, code, args)
    }

    #[inline]
    pub fn admission_ticket(&self) -> Option<u64> {
        let word = self.control.load(Ordering::Acquire);
        if word & ACCEPTING != 0 {
            Some(word)
        } else {
            None
        }
    }

    /// Capture the ticket before reading the target clock/thread registers, so
    /// a producer suspended there cannot carry an old timestamp across resume.
    #[inline]
    pub fn record_ticket(
        &self,
        ticket: u64,
        tick: u64,
        thread: u64,
        code: u64,
        args: [u64; 5],
    ) -> bool {
        self.record_with(
            ticket,
            [tick, thread, code, args[0], args[1], args[2], args[3], args[4]],
            || {},
            || {},
        )
    }

    #[inline]
    fn record_with(
        &self,
        ticket: u64,
        fields: [u64; 8],
        before_lock: impl FnOnce(),
        after_admission: impl FnOnce(),
    ) -> bool {
        if ticket & ACCEPTING == 0 {
            return false;
        }
        before_lock();
        let Some(mut guard) = self.lock() else {
            self.busy.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        let current = self.control.load(Ordering::Acquire);
        if current & ACCEPTING == 0 || (current ^ ticket) & GENERATION_MASK != 0 {
            return false;
        }
        // Admission linearizes at the load above while holding the FIFO lock.
        // Stop does not wait for this producer: the writer subsequently acquires
        // the same lock before reporting a drained/sealed pause. State changes
        // before admission reject the row; changes after admission drain it.
        after_admission();
        let inner = guard.inner();
        if inner.accepted - inner.drained == N as u64 {
            self.full.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = (inner.accepted % N as u64) as usize;
        let seq = inner.accepted + 1;
        inner.records[slot] = [
            seq, fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], fields[6], fields[7],
            0,
        ];
        inner.accepted = seq;
        true
    }

    /// One consumer only. Empty and contended both mean "try later".
    pub fn pop(&self) -> Option<Record> {
        let mut guard = self.lock()?;
        let inner = guard.inner();
        if inner.drained == inner.accepted {
            return None;
        }
        let row = inner.records[(inner.drained % N as u64) as usize];
        inner.drained += 1;
        Some(row)
    }

    pub fn stats(&self) -> Option<Stats> {
        let mut guard = self.lock()?;
        let inner = guard.inner();
        Some(Stats {
            accepted: inner.accepted,
            drained: inner.drained,
            busy: self.busy.load(Ordering::Relaxed),
            full: self.full.load(Ordering::Relaxed),
        })
    }

    pub fn status(&self) -> u64 {
        self.control.load(Ordering::Acquire) & STATE_MASK
    }

    /// UI entry point: fixed atomic counters, no queue lock, allocation or IO.
    /// Requests are idempotent. Start during Stopping returns Stopping; callers
    /// must wait for Paused before asking again. No implicit deferred reopen.
    pub fn command(&self, command: u32) -> u64 {
        match command {
            0 => self.status(),
            1 | 2 => {
                let mut word = self.control.load(Ordering::Acquire);
                loop {
                    let state = word & STATE_MASK;
                    let next = match (command, state) {
                        (1, PAUSED) => {
                            ((word & GENERATION_MASK).wrapping_add(GENERATION)) | STARTING
                        }
                        (2, STARTING | RECORDING) => (word & GENERATION_MASK) | STOPPING,
                        _ => return state,
                    };
                    // A competing writer/request can cause a retry, but this
                    // never waits for the FIFO lock or disk/producer completion.
                    match self.control.compare_exchange(
                        word,
                        next,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => return next & STATE_MASK,
                        Err(changed) => word = changed,
                    }
                }
            }
            3 => self.persisted_bytes.load(Ordering::Acquire),
            4 => DISK_CAP,
            5 => self
                .busy
                .load(Ordering::Relaxed)
                .saturating_add(self.full.load(Ordering::Relaxed)),
            6 => self.last_error.load(Ordering::Acquire),
            _ => u64::MAX,
        }
    }

    pub fn close(&self) {
        self.command(2);
    }

    /// Single writer only. Stop winning this CAS leaves admission closed.
    fn recording(&self) {
        let word = self.control.load(Ordering::Acquire);
        if word & STATE_MASK == STARTING {
            let _ = self.control.compare_exchange(
                word,
                (word & GENERATION_MASK) | RECORDING | ACCEPTING,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    fn paused(&self) {
        let word = self.control.load(Ordering::Acquire);
        // Start cannot modify Stopping. Only the writer can publish Paused,
        // and only after all admitted rows and the pause marker are flushed.
        debug_assert_eq!(word & STATE_MASK, STOPPING);
        self.control
            .store((word & GENERATION_MASK) | PAUSED, Ordering::Release);
    }

    pub fn fail(&self, error: Error) {
        self.last_error.store(error.code(), Ordering::Release);
        self.terminal(if error == Error::Capped {
            STATUS_CAPPED
        } else {
            STATUS_ERROR
        });
    }

    fn terminal(&self, state: u64) {
        // No request accepts a terminal state; no future producer is admitted.
        self.control.store(state, Ordering::Release);
    }

    /// Only the worker may wait for a producer's bounded FIFO critical section.
    fn writer_stats(&self) -> Stats {
        loop {
            if let Some(stats) = self.stats() {
                return stats;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[cfg(test)]
    #[allow(dead_code)] // Used by the standalone recorder harness, not NRO lib tests.
    pub fn test_hold_lock(&self) -> impl Drop + '_ {
        self.lock().expect("test starts with free queue")
    }

    #[cfg(test)]
    #[allow(dead_code)] // Used by the standalone control harness.
    pub fn test_record_hooks(
        &self,
        before_lock: impl FnOnce(),
        after_admission: impl FnOnce(),
    ) -> bool {
        let Some(ticket) = self.admission_ticket() else {
            return false;
        };
        self.record_with(ticket, [1, 2, 3, 4, 4, 4, 4, 4], before_lock, after_admission)
    }
}

impl<const N: usize> Default for Queue<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// All calls run on the worker. read must fill the supplied slice or fail;
/// write_flush must report errors, including short writes/flush failures.
pub trait Storage {
    fn length(&mut self) -> Result<u64, u64>;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), u64>;
    fn grow(&mut self, size: u64) -> Result<(), u64>;
    fn write_flush(&mut self, offset: u64, bytes: &[u8]) -> Result<(), u64>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Io(u64),
    InvalidHeader,
    InvalidTail,
    Oversize,
    Capped,
    ReservedFinalSlot,
    InvalidRecordCount,
}

impl Error {
    /// Stable synthetic control codes; SDK/OS errors retain their raw value.
    pub fn code(self) -> u64 {
        match self {
            Self::Io(code) => code,
            Self::InvalidHeader => 0x8000_0000_0000_0010,
            Self::InvalidTail => 0x8000_0000_0000_0011,
            Self::Oversize => 0x8000_0000_0000_0012,
            Self::Capped => 0, // Cap is a normal terminal outcome, not an IO error.
            Self::ReservedFinalSlot => 0x8000_0000_0000_0013,
            Self::InvalidRecordCount => 0x8000_0000_0000_0014,
        }
    }
}

pub fn put(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
pub fn get(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("fixed schema field"))
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut v = i as u32;
        let mut bit = 0;
        while bit < 8 {
            v = (v >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(v & 1));
            bit += 1;
        }
        table[i] = v;
        i += 1;
    }
    table
}
const CRC_TABLE: [u32; 256] = crc_table();
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ u32::from(*byte)) & 255) as usize];
    }
    !crc
}
fn seal(bytes: &mut [u8; CHUNK_BYTES], slot: u64) {
    bytes[CHUNK_BYTES - 16..CHUNK_BYTES - 12].copy_from_slice(b"END2");
    put(bytes, CHUNK_BYTES - 12, slot);
    let crc = crc32(&bytes[..CHUNK_BYTES - 4]);
    bytes[CHUNK_BYTES - 4..].copy_from_slice(&crc.to_le_bytes());
}
fn sealed(bytes: &[u8; CHUNK_BYTES], slot: u64) -> bool {
    bytes[CHUNK_BYTES - 16..CHUNK_BYTES - 12] == *b"END2"
        && get(bytes, CHUNK_BYTES - 12) == slot
        && bytes[CHUNK_BYTES - 4..] == crc32(&bytes[..CHUNK_BYTES - 4]).to_le_bytes()
}
fn file_header(bytes: &mut [u8; CHUNK_BYTES]) {
    bytes.fill(0);
    bytes[..8].copy_from_slice(b"PTRFILE2");
    for (i, v) in [
        2,
        CHUNK_BYTES as u64,
        RECORD_BYTES as u64,
        DISK_CAP,
        ROWS_PER_CHUNK as u64,
        CHUNK_HEADER as u64,
        1, // IEEE CRC-32
    ]
    .iter()
    .enumerate()
    {
        put(bytes, 8 + i * 8, *v);
    }
    seal(bytes, 0);
}
fn valid_header(bytes: &[u8; CHUNK_BYTES]) -> bool {
    bytes[..8] == *b"PTRFILE2"
        && get(bytes, 8) == 2
        && get(bytes, 16) == CHUNK_BYTES as u64
        && get(bytes, 24) == RECORD_BYTES as u64
        && get(bytes, 32) == DISK_CAP
        && get(bytes, 40) == ROWS_PER_CHUNK as u64
        && get(bytes, 48) == CHUNK_HEADER as u64
        && get(bytes, 56) == 1
        && sealed(bytes, 0)
}
pub fn valid_chunk(bytes: &[u8; CHUNK_BYTES], slot: u64) -> bool {
    bytes[..8] == *b"PTRCHK02"
        && get(bytes, 8) == 2
        && get(bytes, 16) == slot
        && get(bytes, 24) > 0
        && get(bytes, 24) <= slot
        && (SESSION..=PAUSE_END).contains(&get(bytes, 32))
        && get(bytes, 40) <= ROWS_PER_CHUNK as u64
        && sealed(bytes, slot)
}

pub fn encode_row(bytes: &mut [u8; CHUNK_BYTES], index: usize, row: Record) {
    assert!(index < ROWS_PER_CHUNK);
    for (j, word) in row.iter().enumerate() {
        put(bytes, CHUNK_HEADER + index * RECORD_BYTES + j * 8, *word);
    }
}

#[derive(Default)]
pub struct Metadata {
    pub kind: u64,
    pub tick: u64,
    pub main: u64,
    pub stats: Stats,
    pub abandoned: u64,
    pub live: u64,
    pub peak: u64,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_results: u64,
    pub tick_hz: u64,
}

/// A session's physical cursor; there is no seek-back/rewrite operation.
pub struct Stream {
    pub next_slot: u64,
    pub persisted_bytes: u64,
    pub session: u64,
    pub recovery_offset: u64,
    pub recovery_bytes: u64,
    pub write_failures: u64,
    pub last_error: u64,
    pub failed_slot: u64,
    pub failed_records: u64,
    build: [u8; 32],
}

impl Stream {
    /// Checks at most 3 × 64 KiB. An unsealed last slot is retained byte-for-byte;
    /// its predecessor must be sealed. Never scans historical payload at boot.
    pub fn open(
        storage: &mut impl Storage,
        bytes: &mut [u8; CHUNK_BYTES],
        build: [u8; 32],
    ) -> Result<Self, Error> {
        let mut len = storage.length().map_err(Error::Io)?;
        if len > DISK_CAP {
            return Err(Error::Oversize);
        }
        if len == 0 {
            file_header(bytes);
            storage.grow(CHUNK_BYTES as u64).map_err(Error::Io)?;
            storage.write_flush(0, bytes).map_err(Error::Io)?;
            len = CHUNK_BYTES as u64;
        } else {
            if len < CHUNK_BYTES as u64 {
                return Err(Error::InvalidHeader);
            }
            storage.read(0, bytes).map_err(Error::Io)?;
            if !valid_header(bytes) {
                return Err(Error::InvalidHeader);
            }
        }
        let last = (len - 1) / CHUNK_BYTES as u64;
        let mut recovery_offset = 0;
        let mut recovery_bytes = 0;
        if last > 0 {
            let tail_bytes = len - last * CHUNK_BYTES as u64;
            let tail_valid = if tail_bytes == CHUNK_BYTES as u64 {
                storage
                    .read(last * CHUNK_BYTES as u64, bytes)
                    .map_err(Error::Io)?;
                valid_chunk(bytes, last)
            } else {
                false
            };
            if tail_valid {
                if get(bytes, 32) == CAPPED {
                    return Err(Error::Capped);
                }
            } else {
                recovery_offset = last * CHUNK_BYTES as u64;
                recovery_bytes = tail_bytes;
                if last > 1 {
                    storage
                        .read((last - 1) * CHUNK_BYTES as u64, bytes)
                        .map_err(Error::Io)?;
                    if !valid_chunk(bytes, last - 1) || get(bytes, 32) == CAPPED {
                        return Err(Error::InvalidTail);
                    }
                }
            }
        }
        let next_slot = last + 1;
        if next_slot >= SLOT_COUNT {
            return Err(Error::Capped);
        }
        bytes.fill(0);
        Ok(Self {
            next_slot,
            persisted_bytes: len,
            session: next_slot,
            recovery_offset,
            recovery_bytes,
            write_failures: 0,
            last_error: 0,
            failed_slot: 0,
            failed_records: 0,
            build,
        })
    }

    pub fn final_slot(&self) -> bool {
        self.next_slot == SLOT_COUNT - 1
    }

    /// Reserves a physical slot before IO. An error never rewinds that cursor;
    /// a subsequent failure marker may use the next slot, never the damaged one.
    pub fn append(
        &mut self,
        storage: &mut impl Storage,
        bytes: &mut [u8; CHUNK_BYTES],
        meta: &Metadata,
        count: usize,
    ) -> Result<(), Error> {
        if count > ROWS_PER_CHUNK {
            return Err(Error::InvalidRecordCount);
        }
        if self.next_slot >= SLOT_COUNT {
            return Err(Error::Capped);
        }
        if self.final_slot() && !matches!(meta.kind, CAPPED | WRITE_FAILED) {
            return Err(Error::ReservedFinalSlot);
        }
        let slot = self.next_slot;
        bytes[..CHUNK_HEADER].fill(0);
        bytes[..8].copy_from_slice(b"PTRCHK02");
        for (i, v) in [
            2,
            slot,
            self.session,
            meta.kind,
            count as u64,
            meta.tick,
            meta.main,
            meta.stats.accepted,
            meta.stats.drained,
            meta.stats.busy,
            meta.stats.full,
            meta.stats.queued(),
            meta.abandoned,
            self.write_failures,
            self.last_error,
            self.recovery_offset,
            self.recovery_bytes,
            meta.live,
            meta.peak,
            meta.memory_total,
            meta.memory_used,
            meta.memory_results,
            meta.tick_hz,
        ]
        .iter()
        .enumerate()
        {
            put(bytes, 8 + i * 8, *v);
        }
        bytes[192..224].copy_from_slice(&self.build);
        put(bytes, 224, self.failed_records);
        put(bytes, 232, QUEUE_CAPACITY as u64);
        put(bytes, 240, FLUSH_MS);
        put(bytes, 248, self.failed_slot);
        bytes[CHUNK_HEADER + count * RECORD_BYTES..CHUNK_BYTES - 16].fill(0);
        seal(bytes, slot);
        self.next_slot += 1;
        let result = storage
            .grow(self.next_slot * CHUNK_BYTES as u64)
            .and_then(|()| storage.write_flush(slot * CHUNK_BYTES as u64, bytes));
        if let Err(code) = result {
            self.write_failures += 1;
            self.last_error = code;
            self.failed_slot = slot;
            self.failed_records += count as u64;
            return Err(Error::Io(code));
        }
        self.persisted_bytes = self.next_slot * CHUNK_BYTES as u64;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Idle,
    Flushed,
    Finished,
}

/// Single-writer session state, shared by the target adapter and pure-host tests.
/// All methods here (unlike Queue::command/record) belong to the writer thread.
/// The caller owns one static output chunk; this struct never allocates a buffer.
pub struct Recorder {
    pub stream: Stream,
    pub failure_marker: Option<Result<(), Error>>,
    count: usize,
    persisted_drops: (u64, u64),
    // The last sealed capture boundary. A Start canceled before service must
    // not emit another pause marker or consume a new disk slot.
    pause_sealed: bool,
}

impl Recorder {
    pub fn open<const N: usize>(
        storage: &mut impl Storage,
        bytes: &mut [u8; CHUNK_BYTES],
        build: [u8; 32],
        queue: &Queue<N>,
        mut metadata: impl FnMut() -> Metadata,
    ) -> Result<Self, Error> {
        let opened = storage.length().map_err(Error::Io).and_then(|len| {
            // Existing physical extent is reported even when append is refused.
            // Thereafter this counter advances only after successful flushes;
            // a failed write may have extended the physical file further.
            queue.persisted_bytes.store(len, Ordering::Release);
            Stream::open(storage, bytes, build)
        });
        let stream = match opened {
            Ok(stream) => stream,
            Err(error) => {
                queue.fail(error);
                return Err(error);
            }
        };
        queue
            .persisted_bytes
            .store(stream.persisted_bytes, Ordering::Release);
        let mut recorder = Self {
            stream,
            failure_marker: None,
            count: 0,
            persisted_drops: (0, 0),
            pause_sealed: false,
        };
        if !recorder.stream.final_slot() {
            recorder.append(storage, bytes, queue, SESSION, &mut metadata)?;
            queue.recording();
        }
        Ok(recorder)
    }

    fn append<const N: usize>(
        &mut self,
        storage: &mut impl Storage,
        bytes: &mut [u8; CHUNK_BYTES],
        queue: &Queue<N>,
        kind: u64,
        metadata: &mut impl FnMut() -> Metadata,
    ) -> Result<(), Error> {
        let mut meta = metadata();
        meta.kind = kind;
        meta.stats = queue.writer_stats();
        meta.abandoned = if kind == CAPPED {
            meta.stats.queued()
        } else {
            0
        };
        match self.stream.append(storage, bytes, &meta, self.count) {
            Ok(()) => {
                queue
                    .persisted_bytes
                    .store(self.stream.persisted_bytes, Ordering::Release);
                self.persisted_drops = (meta.stats.busy, meta.stats.full);
                self.count = 0;
                bytes.fill(0);
                Ok(())
            }
            Err(error) => {
                queue.fail(error);
                // Acquiring the FIFO lock after closing admission observes the
                // last admitted producer. Failed bytes are never overwritten.
                let mut failure = metadata();
                failure.kind = WRITE_FAILED;
                failure.stats = queue.writer_stats();
                failure.abandoned = failure.stats.queued() + self.stream.failed_records;
                let result = self.stream.append(storage, bytes, &failure, 0);
                if let Err(marker_error) = result {
                    // Exhausting the marker slot must not turn a write error into
                    // ordinary Capped. Keep Error and the last actual IO error.
                    if let Error::Io(_) = marker_error {
                        queue.fail(marker_error);
                    }
                } else {
                    queue
                        .persisted_bytes
                        .store(self.stream.persisted_bytes, Ordering::Release);
                }
                self.failure_marker = Some(result);
                Err(error)
            }
        }
    }

    pub fn step<const N: usize>(
        &mut self,
        storage: &mut impl Storage,
        bytes: &mut [u8; CHUNK_BYTES],
        queue: &Queue<N>,
        urgent: bool,
        deadline: bool,
        mut metadata: impl FnMut() -> Metadata,
    ) -> Result<Progress, Error> {
        if matches!(queue.status(), STATUS_CAPPED | STATUS_ERROR) {
            return Ok(Progress::Finished);
        }
        let capped = self.stream.final_slot();
        if capped {
            queue.close();
        } else {
            match queue.status() {
                PAUSED => return Ok(Progress::Idle),
                STARTING => {
                    // Boot Starting is handled by open(). This is a resume with
                    // admission still closed: flush the live allocation baseline
                    // before opening a new generation to producers.
                    self.append(storage, bytes, queue, PAUSE_END, &mut metadata)?;
                    self.pause_sealed = false;
                    queue.recording();
                    return Ok(Progress::Flushed);
                }
                STOPPING if self.pause_sealed => {
                    queue.paused();
                    return Ok(Progress::Idle);
                }
                _ => {}
            }
        }
        while self.count < ROWS_PER_CHUNK {
            let Some(row) = queue.pop() else { break };
            encode_row(bytes, self.count, row);
            self.count += 1;
        }
        if capped {
            self.append(storage, bytes, queue, CAPPED, &mut metadata)?;
            queue.terminal(STATUS_CAPPED);
            return Ok(Progress::Finished);
        }
        if queue.status() == STOPPING && queue.writer_stats().queued() == 0 {
            self.append(storage, bytes, queue, PAUSE_BEGIN, &mut metadata)?;
            self.pause_sealed = true;
            queue.paused();
            return Ok(Progress::Flushed);
        }
        let drops_changed = deadline
            && queue
                .stats()
                .map(|s| (s.busy, s.full) != self.persisted_drops)
                .unwrap_or(false);
        if self.count == ROWS_PER_CHUNK
            || ((urgent || deadline) && self.count > 0)
            || drops_changed
        {
            self.append(storage, bytes, queue, EVENTS, &mut metadata)?;
            Ok(Progress::Flushed)
        } else {
            Ok(Progress::Idle)
        }
    }
}

pub fn parse_build_id(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let mut id = [0; 32];
    for (i, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        id[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(id)
}
