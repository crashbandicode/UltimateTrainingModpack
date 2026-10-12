//! Saved-replay import for the 13.0.5 FRAM payload subset decoded by
//! `smash_replay_core`.
//!
//! The caller supplies an extracted FRAM payload, an explicit source version and,
//! optionally, the exact inner setup header and the whole saved replay's content
//! hash. This module never locates either buffer in a saved replay file,
//! validates the outer container or checksum values, or runs any simulation.
//!
//! * A decoded record is a replay record position (`clocks.record`). It is not a
//!   gameplay tick, an input-consumption step or a video frame.
//! * Per controller slot, the stored sparse packet is kept apart from the
//!   decoder's expansion. An absent packet expands to zero plus the header button
//!   bits; the import records that slot as `missing`, never as observed neutral
//!   or `unchanged`.
//! * replay-core exposes each packet's four stored signed-byte axes without
//!   establishing which stick axis each one is. Semantic `left_stick` and
//!   `right_stick` are therefore `unsupported` unless the caller supplies an
//!   [`AxisAssignment`]; the stored axes always stay in the packet's raw bytes.
//! * Every record is also kept verbatim as a `raw` event, so flags, checksum,
//!   timing bit, opaque events, the extra word and offsets survive unchanged.
//! * Fighter state, animation, lifecycle, objects, random outcomes, stage, rules,
//!   costume and human/CPU designation are not decoded and stay unsupported or
//!   absent. The stored setup seed word is not an RNG observation.
//!
//! The saved file therefore supplies far fewer channels than a live producer.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use smash_replay_core::records::{decode_records, DecodeError, DecodeErrorKind, Record};
use smash_replay_core::setup::{
    HeaderError, StoredSetupHeader, HEADER_SIZE, PLAYER_ENTRIES, VERSION_WORD,
};

use crate::capture::{CaptureError, Finished, Recorder, RecorderOptions};
use crate::model::*;

/// The only game build whose FRAM grammar and setup header replay-core supports.
pub const SUPPORTED_SOURCE_VERSION: &str = "13.0.5";
/// Expanded controller slots per record in the supported grammar.
pub const RECORD_SLOTS: usize = 8;
/// Decoder recorded in `source.decoder`.
pub const DECODER_NAME: &str = "smash-replay-core";
/// Version of the `../smash-replay-export/replay-core` path dependency.
pub const DECODER_VERSION: &str = "0.1.0";
/// Input layout id declared for decoded packets.
pub const INPUT_LAYOUT_ID: &str = "fram_13_0_5";
/// `RawBytes` encoding of one stored eight-byte packet.
pub const PACKET_ENCODING: &str = "fram_packet_13_0_5";
/// `RawBytes` encoding of one whole record; `source_offset` is within the payload.
pub const RECORD_ENCODING: &str = "fram_record_13_0_5";
/// `RawBytes` encoding of the exact inner setup header in `rules.actual.raw_setup`.
pub const SETUP_HEADER_ENCODING: &str = "cfmk_inner_header_13_0_5";
/// Digest scope of exactly the supplied FRAM payload bytes.
pub const PAYLOAD_DIGEST_SCOPE: &str = "fram_payload_13_0_5";
/// Digest scope of exactly the supplied inner setup header bytes.
pub const SETUP_HEADER_DIGEST_SCOPE: &str = "cfmk_inner_header_13_0_5";

// ---------------------------------------------------------------------------
// Decoded view
// ---------------------------------------------------------------------------

/// One stored eight-byte packet, exactly as present in the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredPacket {
    /// Byte offset of the packet within the supplied FRAM payload.
    pub payload_offset: usize,
    /// The exact stored bytes.
    pub bytes: [u8; 8],
    /// Raw button word before the header bits are ORed in. Bit meanings are opaque.
    pub buttons: u32,
    /// Four stored signed-byte axes in payload order. Analog scaling is not
    /// established by the decoder.
    pub axes: [i8; 4],
}

/// One controller slot of one record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRecord {
    pub slot: u8,
    /// `Some` only when the record's presence bit for this slot is set.
    pub packet: Option<StoredPacket>,
    /// Header flag bit `16 + slot`, which the decoder ORs into button bit 7.
    pub header_bit7: bool,
    /// Header flag bit `24 + slot`, which the decoder ORs into button bit 15.
    pub header_bit15: bool,
    /// Decoder expansion: the packet word (or zero when absent) with header bits
    /// ORed in. For an absent packet this is not an observation.
    pub expanded_buttons: u32,
    /// Decoder expansion of the axes: the packet's axes, or zero when absent.
    pub expanded_axes: [i8; 4],
}

impl SlotRecord {
    pub fn packet_present(&self) -> bool {
        self.packet.is_some()
    }

    /// The record stores something for this slot (a packet or a header bit).
    pub fn has_stored_data(&self) -> bool {
        self.packet.is_some() || self.header_bit7 || self.header_bit15
    }
}

/// One decoded FRAM record with every raw field the decoder exposes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayRecord {
    /// Zero-based record position within the supplied payload.
    pub ordinal: usize,
    pub payload_offset: usize,
    pub record_size: usize,
    /// The exact record bytes (`payload[payload_offset..][..record_size]`).
    pub bytes: Vec<u8>,
    pub flags: u32,
    /// Stored checksum word; not verified.
    pub checksum: u32,
    /// Raw header bit 7; no game or render clock meaning is assigned.
    pub timing_bit: u8,
    pub slots: [SlotRecord; RECORD_SLOTS],
    /// Opaque event word pairs, in payload order.
    pub events: Vec<[u16; 2]>,
    /// Opaque extra word present when flag bit 6 is set.
    pub extra_word: Option<u32>,
}

impl ReplayRecord {
    fn from_core(record: &Record, payload: &[u8]) -> Self {
        let slots = std::array::from_fn(|slot| {
            let packet = record
                .packets
                .iter()
                .find(|packet| packet.slot == slot)
                .map(|packet| {
                    let mut bytes = [0; 8];
                    bytes.copy_from_slice(
                        &payload[packet.payload_offset..packet.payload_offset + 8],
                    );
                    StoredPacket {
                        payload_offset: packet.payload_offset,
                        bytes,
                        buttons: packet.buttons,
                        axes: packet.sticks,
                    }
                });
            let control = record.controls[slot];
            SlotRecord {
                slot: slot as u8,
                packet,
                header_bit7: record.flags & (1 << (16 + slot)) != 0,
                header_bit15: record.flags & (1 << (24 + slot)) != 0,
                expanded_buttons: control.buttons,
                expanded_axes: control.sticks,
            }
        });
        Self {
            ordinal: record.ordinal,
            payload_offset: record.payload_offset,
            record_size: record.record_size,
            bytes: payload[record.payload_offset..record.payload_offset + record.record_size]
                .to_vec(),
            flags: record.flags,
            checksum: record.checksum,
            timing_bit: record.timing_bit,
            slots,
            events: record.events.clone(),
            extra_word: record.extra_word,
        }
    }
}

/// Values exposed by the stored setup header view. Everything else stays opaque
/// and is kept only as the raw header bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredSetup {
    /// Raw header version word (0xd005 for the supported 13.0.5 path).
    pub version_word: u32,
    /// Stored u32 at inner +0xf0. A saved setup value, not an observed RNG state.
    pub stored_seed_word: u32,
    /// Raw native fighter kinds per saved player entry (entry +8 key expansion),
    /// indexed by entry. Whether native setup creates an entry is not decoded.
    pub entry_fighter_kinds: Vec<Vec<i32>>,
}

impl StoredSetup {
    fn from_header(header: &StoredSetupHeader<'_>) -> Self {
        Self {
            version_word: VERSION_WORD,
            stored_seed_word: header.stored_seed_word(),
            entry_fighter_kinds: (0..PLAYER_ENTRIES)
                .filter_map(|index| header.player_fighter_kinds(index))
                .map(|kinds| kinds.as_slice().to_vec())
                .collect(),
        }
    }
}

/// Everything decoded from the supplied buffers, before mapping to a capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedReplay {
    pub records: Vec<ReplayRecord>,
    pub setup: Option<StoredSetup>,
    /// Digest of exactly the supplied FRAM payload ([`PAYLOAD_DIGEST_SCOPE`]).
    pub payload_digest: ContentDigest,
    /// Digest of exactly the supplied inner header ([`SETUP_HEADER_DIGEST_SCOPE`]).
    pub setup_digest: Option<ContentDigest>,
}

impl DecodedReplay {
    /// Slots that store a packet or a header button bit in at least one record.
    pub fn active_slots(&self) -> Vec<u8> {
        (0..RECORD_SLOTS)
            .filter(|&slot| self.records.iter().any(|r| r.slots[slot].has_stored_data()))
            .map(|slot| slot as u8)
            .collect()
    }

    pub fn packet_count(&self) -> usize {
        self.records
            .iter()
            .map(|r| r.slots.iter().filter(|s| s.packet_present()).count())
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Caller-asserted stick axes
// ---------------------------------------------------------------------------

/// Caller-asserted assignment of a packet's four stored axis bytes (indices
/// 0..=3, in payload order) to semantic stick axes. replay-core does not
/// establish this mapping; an import without an assignment leaves semantic
/// sticks `unsupported`. Always a permutation of 0, 1, 2, 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AxisAssignment {
    left_x: u8,
    left_y: u8,
    right_x: u8,
    right_y: u8,
}

impl AxisAssignment {
    /// Stored axis index for each semantic axis; must be a permutation of 0..=3.
    pub fn new(
        left_x: u8,
        left_y: u8,
        right_x: u8,
        right_y: u8,
    ) -> Result<Self, ReplayImportError> {
        let mut seen = [false; 4];
        for index in [left_x, left_y, right_x, right_y] {
            match seen.get_mut(usize::from(index)) {
                Some(seen) if !*seen => *seen = true,
                _ => {
                    return Err(ReplayImportError::InvalidAxisAssignment {
                        supplied: format!("{left_x},{left_y},{right_x},{right_y}"),
                    })
                }
            }
        }
        Ok(Self {
            left_x,
            left_y,
            right_x,
            right_y,
        })
    }

    /// Stored axis indices as `[left_x, left_y, right_x, right_y]`.
    pub fn order(&self) -> [u8; 4] {
        [self.left_x, self.left_y, self.right_x, self.right_y]
    }

    /// Semantic `(left, right)` sticks from one packet's stored axes.
    pub fn sticks(&self, axes: [i8; 4]) -> (Stick, Stick) {
        let at = |index: u8| axes[usize::from(index)];
        (
            Stick::I8 {
                x: at(self.left_x),
                y: at(self.left_y),
            },
            Stick::I8 {
                x: at(self.right_x),
                y: at(self.right_y),
            },
        )
    }
}

impl fmt::Display for AxisAssignment {
    /// `left_x,left_y,right_x,right_y`, the form accepted by `from_str`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.left_x, self.left_y, self.right_x, self.right_y
        )
    }
}

impl FromStr for AxisAssignment {
    type Err = ReplayImportError;

    /// Parse `left_x,left_y,right_x,right_y`, for example `0,1,2,3`.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || ReplayImportError::InvalidAxisAssignment {
            supplied: value.to_string(),
        };
        let indices = value
            .split(',')
            .map(|part| part.trim().parse::<u8>())
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| invalid())?;
        let [left_x, left_y, right_x, right_y] =
            <[u8; 4]>::try_from(indices).map_err(|_| invalid())?;
        Self::new(left_x, left_y, right_x, right_y).map_err(|_| invalid())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Import failures. Nothing is imported when any of these is returned.
#[derive(Debug)]
pub enum ReplayImportError {
    /// The caller must assert exactly [`SUPPORTED_SOURCE_VERSION`].
    UnsupportedSourceVersion { supplied: String },
    /// The FRAM payload failed to decode; `payload_offset` is within the payload.
    Records(DecodeError),
    /// The supplied inner setup header was rejected.
    SetupHeader(HeaderError),
    /// The whole-replay association is not a 64-digit hex SHA-256.
    InvalidReplaySha256 { value: String },
    /// The whole-replay association equals the digest of an extracted buffer,
    /// so it does not identify the saved replay file.
    ReplayShaIsExtractedDigest { scope: &'static str },
    /// A caller axis assignment is not a permutation of the stored axes 0..=3.
    InvalidAxisAssignment { supplied: String },
    /// The recorder rejected the mapped capture or could not write it.
    Capture(CaptureError),
}

impl fmt::Display for ReplayImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSourceVersion { supplied } => write!(
                f,
                "unsupported replay source version {supplied:?}; only {SUPPORTED_SOURCE_VERSION} is supported"
            ),
            Self::Records(error) => {
                let what = match error.kind {
                    DecodeErrorKind::TruncatedHeader => "truncated record header".to_string(),
                    DecodeErrorKind::UnsupportedStopMarker => {
                        "unsupported stop marker (flag 0x20)".to_string()
                    }
                    DecodeErrorKind::TruncatedPacket { slot } => {
                        format!("truncated packet for slot {slot}")
                    }
                    DecodeErrorKind::TruncatedEvent => "truncated event".to_string(),
                    DecodeErrorKind::TruncatedExtraWord => "truncated extra word".to_string(),
                };
                write!(
                    f,
                    "FRAM payload: {what} at payload offset {0} ({0:#x}); nothing imported",
                    error.payload_offset
                )
            }
            Self::SetupHeader(error) => match error {
                HeaderError::Length { actual } => write!(
                    f,
                    "setup header: expected exactly {HEADER_SIZE:#x} bytes, got {actual:#x}"
                ),
                HeaderError::Magic => f.write_str("setup header: CFMK magic not found at offset 0x4"),
                HeaderError::Version { found } => write!(
                    f,
                    "setup header: unsupported version word {found:#x} at offset 0x8 (expected {VERSION_WORD:#x})"
                ),
                HeaderError::DeclaredLength { found } => write!(
                    f,
                    "setup header: declared size {found:#x} at offset 0x10 (expected {HEADER_SIZE:#x})"
                ),
            },
            Self::InvalidReplaySha256 { value } => write!(
                f,
                "saved-replay sha256 {value:?} is not 64 hexadecimal digits"
            ),
            Self::ReplayShaIsExtractedDigest { scope } => write!(
                f,
                "saved-replay sha256 equals the {scope} digest; it must hash the whole saved replay file"
            ),
            Self::InvalidAxisAssignment { supplied } => write!(
                f,
                "axis assignment {supplied:?} must give the stored axis index (0-3) for \
                 left_x,left_y,right_x,right_y as a permutation of 0,1,2,3"
            ),
            Self::Capture(error) => write!(f, "capture rejected the import: {error}"),
        }
    }
}

impl std::error::Error for ReplayImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Capture(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CaptureError> for ReplayImportError {
    fn from(error: CaptureError) -> Self {
        Self::Capture(error)
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Check the caller's explicit source version assertion.
pub fn check_source_version(source_version: &str) -> Result<(), ReplayImportError> {
    if source_version == SUPPORTED_SOURCE_VERSION {
        Ok(())
    } else {
        Err(ReplayImportError::UnsupportedSourceVersion {
            supplied: source_version.to_string(),
        })
    }
}

/// Decode an extracted FRAM payload into raw replay records.
pub fn decode_payload(
    source_version: &str,
    fram_payload: &[u8],
) -> Result<Vec<ReplayRecord>, ReplayImportError> {
    check_source_version(source_version)?;
    let records = decode_records(fram_payload).map_err(ReplayImportError::Records)?;
    Ok(records
        .iter()
        .map(|record| ReplayRecord::from_core(record, fram_payload))
        .collect())
}

/// Parse an exact inner setup header into its exposed stored values.
pub fn decode_setup_header(
    source_version: &str,
    header: &[u8],
) -> Result<StoredSetup, ReplayImportError> {
    check_source_version(source_version)?;
    let header = StoredSetupHeader::parse(header).map_err(ReplayImportError::SetupHeader)?;
    Ok(StoredSetup::from_header(&header))
}

fn content_digest(scope: &str, bytes: &[u8]) -> ContentDigest {
    ContentDigest {
        scope: scope.to_string(),
        sha256: sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
    }
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// What the caller supplies for one import. Nothing here is guessed.
#[derive(Clone, Debug)]
pub struct ReplayImportRequest<'a> {
    pub capture_id: String,
    /// Game version the caller asserts for the extracted buffers. The decoder
    /// requires an externally version-checked payload; only 13.0.5 is accepted.
    pub source_version: String,
    /// The extracted FRAM payload only, not the saved replay file.
    pub fram_payload: &'a [u8],
    /// The exact 0x1bb0-byte inner setup header, starting at its checksum word.
    pub setup_header: Option<&'a [u8]>,
    /// Association with the whole saved replay file (its SHA-256), e.g. from
    /// [`ReplayAssociation::from_bytes`] over the file. The link between that
    /// file and the extracted buffers is the caller's assertion.
    pub replay: Option<ReplayAssociation>,
    /// Intended ruleset; the replay's actual rules are not decoded.
    pub rules_target: RulesetTarget,
    pub created_utc: Option<String>,
    /// Caller-asserted stick axis assignment. `None` (default) keeps semantic
    /// sticks `unsupported`; the stored axes stay in raw bytes either way.
    pub axis_assignment: Option<AxisAssignment>,
}

impl<'a> ReplayImportRequest<'a> {
    pub fn new(
        capture_id: impl Into<String>,
        source_version: impl Into<String>,
        fram_payload: &'a [u8],
    ) -> Self {
        Self {
            capture_id: capture_id.into(),
            source_version: source_version.into(),
            fram_payload,
            setup_header: None,
            replay: None,
            rules_target: RulesetTarget::Unspecified,
            created_utc: None,
            axis_assignment: None,
        }
    }

    pub fn with_setup_header(mut self, header: &'a [u8]) -> Self {
        self.setup_header = Some(header);
        self
    }

    pub fn with_replay(mut self, replay: ReplayAssociation) -> Self {
        self.replay = Some(replay);
        self
    }

    pub fn with_rules_target(mut self, target: RulesetTarget) -> Self {
        self.rules_target = target;
        self
    }

    pub fn with_created_utc(mut self, created_utc: impl Into<String>) -> Self {
        self.created_utc = Some(created_utc.into());
        self
    }

    pub fn with_axis_assignment(mut self, assignment: AxisAssignment) -> Self {
        self.axis_assignment = Some(assignment);
        self
    }
}

/// Validate the request and decode both buffers. Fails before anything is written.
pub fn decode_replay(
    request: &ReplayImportRequest<'_>,
) -> Result<DecodedReplay, ReplayImportError> {
    check_source_version(&request.source_version)?;
    let records = decode_payload(&request.source_version, request.fram_payload)?;
    let setup = request
        .setup_header
        .map(|header| decode_setup_header(&request.source_version, header))
        .transpose()?;
    let payload_digest = content_digest(PAYLOAD_DIGEST_SCOPE, request.fram_payload);
    let setup_digest = request
        .setup_header
        .map(|header| content_digest(SETUP_HEADER_DIGEST_SCOPE, header));
    if let Some(replay) = &request.replay {
        let sha = &replay.sha256;
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ReplayImportError::InvalidReplaySha256 { value: sha.clone() });
        }
        let sha = sha.to_ascii_lowercase();
        if sha == payload_digest.sha256 {
            return Err(ReplayImportError::ReplayShaIsExtractedDigest {
                scope: PAYLOAD_DIGEST_SCOPE,
            });
        }
        if setup_digest.as_ref().is_some_and(|d| d.sha256 == sha) {
            return Err(ReplayImportError::ReplayShaIsExtractedDigest {
                scope: SETUP_HEADER_DIGEST_SCOPE,
            });
        }
    }
    Ok(DecodedReplay {
        records,
        setup,
        payload_digest,
        setup_digest,
    })
}

/// The input layout declared for decoded packets. Stick notes record whether a
/// caller-asserted axis assignment was used.
pub fn input_layout(axes: Option<AxisAssignment>) -> InputLayout {
    let axis = |indices: Option<(u8, u8)>| AxisLayout {
        encoding: AxisEncoding::I8,
        min: None,
        max: None,
        neutral: None,
        float_scale: None,
        note: Some(match indices {
            Some((x, y)) => format!(
                "caller-asserted, not established by replay-core: x = stored axis {x} (packet \
                 byte {}), y = stored axis {y} (packet byte {}); analog scaling, neutral and \
                 used range are not established",
                4 + x,
                4 + y
            ),
            None => "unsupported: replay-core does not establish which stored axes belong to \
                     this stick and no caller assignment was supplied; the four stored axes \
                     stay in packet bytes 4-7"
                .to_string(),
        }),
    };
    InputLayout {
        id: INPUT_LAYOUT_ID.to_string(),
        description: Some(
            "smash-replay-core 13.0.5 FRAM control slot: 32-bit button word (packet word with \
             header bits 7 and 15 ORed in; meanings opaque) and four stored signed-byte axes \
             (packet bytes 4-7, stick assignment not established)"
                .to_string(),
        ),
        buttons: ButtonLayout {
            width_bits: 32,
            names: Default::default(),
        },
        left_stick: axis(axes.map(|a| (a.left_x, a.left_y))),
        right_stick: axis(axes.map(|a| (a.right_x, a.right_y))),
        raw_encoding: Some(PACKET_ENCODING.to_string()),
    }
}

/// Build the manifest header for a decoded replay. Only decoded or caller
/// supplied values are set; everything else stays unsupported or absent.
pub fn capture_header(request: &ReplayImportRequest<'_>, decoded: &DecodedReplay) -> CaptureHeader {
    let active = decoded.active_slots();
    let replay = request.replay.clone().map(|mut replay| {
        replay.sha256 = replay.sha256.to_ascii_lowercase();
        if let Some(setup) = &decoded.setup {
            replay.header_version_word = Some(setup.version_word);
            replay.stored_seed_word = Some(setup.stored_seed_word);
        }
        replay
    });
    let mut digests = vec![decoded.payload_digest.clone()];
    digests.extend(decoded.setup_digest.clone());

    let mut notes = vec![
        format!(
            "Offline decode of a caller-extracted FRAM payload by {DECODER_NAME} {DECODER_VERSION}. \
             The caller asserted source version {SUPPORTED_SOURCE_VERSION}; the decoder does not \
             version-check the payload, the outer container or checksum values."
        ),
        "Clock `record` is the record ordinal within the supplied payload. It is not a gameplay \
         tick, an input-consumption step or a video frame; no relationship is established, and \
         recording start, first playable frame and match end are not decoded."
            .to_string(),
        "Input `controller` is the FRAM control slot (0-7). Its mapping to physical ports, fighter \
         entries, actors and human/CPU designation is not decoded; inputs carry no actor."
            .to_string(),
        "A slot without a stored packet in a record is `missing` (buttons, and sticks when an \
         axis assignment is supplied): the decoder's zero-filled expansion is neither observed \
         neutral nor unchanged. Header button bits 7/15 of such slots stay in that record's raw \
         unit."
            .to_string(),
        match request.axis_assignment {
            Some(axes) => format!(
                "Semantic sticks use the caller-asserted axis assignment \
                 left_x,left_y,right_x,right_y = {axes} (stored axis indices); replay-core does \
                 not establish it. The stored axes also stay in each packet's raw bytes 4-7."
            ),
            None => "Semantic sticks are unsupported: replay-core exposes four stored signed-byte \
                     axes per packet without establishing which stick axis each is, and no caller \
                     assignment was supplied. The stored axes stay in each packet's raw bytes 4-7."
                .to_string(),
        },
        "Each record is kept verbatim as a `raw` event (fram_record_13_0_5, source_offset within \
         the payload); flags, checksum, timing bit 7, events and the extra word remain opaque."
            .to_string(),
    ];
    if decoded.records.is_empty() {
        notes.push("The supplied payload contained no records.".to_string());
    }
    let inactive: Vec<u8> = (0..RECORD_SLOTS as u8)
        .filter(|s| !active.contains(s))
        .collect();
    if !inactive.is_empty() {
        notes.push(format!(
            "FRAM slots {inactive:?} store no packet or header bit in any record and have no input events."
        ));
    }
    match &decoded.setup {
        Some(setup) => {
            notes.push(format!(
                "Stored setup seed word {:#010x} (inner header +0xf0) is a saved setup value, not an \
                 observed RNG state; no random outcome is recorded.",
                setup.stored_seed_word
            ));
            notes.push(
                "Entry fighter-kind groups come from saved entry +8 keys; whether native setup \
                 creates each entry is not decoded. Costume, controller, stage and rules are not \
                 decoded; the exact header is kept in rules.actual.raw_setup."
                    .to_string(),
            );
        }
        None => notes.push("No setup header was supplied: roster and seed are absent.".to_string()),
    }
    if replay.is_none() {
        notes.push(
            "No whole saved-replay SHA-256 was supplied; source.digests identify only the \
             extracted buffers."
                .to_string(),
        );
    }

    CaptureHeader {
        capture_id: request.capture_id.clone(),
        created_utc: request.created_utc.clone(),
        recorder: ToolVersion::this_crate(),
        game: GameBuild::ultimate(SUPPORTED_SOURCE_VERSION),
        source: Source {
            kind: SourceKind::FileDecode,
            description: Some(format!(
                "{SUPPORTED_SOURCE_VERSION} FRAM payload decode{}{}; no live game state was observed",
                if decoded.setup.is_some() {
                    " with inner setup header"
                } else {
                    ""
                },
                match request.axis_assignment {
                    Some(axes) => format!(", caller-asserted stick axis order {axes}"),
                    None => String::new(),
                }
            )),
            replay,
            decoder: Some(ToolVersion::new(DECODER_NAME, DECODER_VERSION)),
            digests,
        },
        rules: Rules {
            target: request.rules_target,
            actual: ActualRules {
                raw_setup: request
                    .setup_header
                    .map(|header| RawBytes::new(SETUP_HEADER_ENCODING, header)),
                ..Default::default()
            },
        },
        stage: Stage::default(),
        controllers: active
            .iter()
            .map(|&slot| ControllerInfo {
                slot,
                kind: None,
                human: None,
                label: None,
            })
            .collect(),
        entries: decoded
            .setup
            .iter()
            .flat_map(|setup| setup.entry_fighter_kinds.iter().enumerate())
            .map(|(entry, kinds)| FighterEntry {
                fighter_kind_group: kinds.clone(),
                ..FighterEntry::new(entry as u8)
            })
            .collect(),
        input_layouts: vec![input_layout(request.axis_assignment)],
        channels: ChannelSupport {
            inputs: Supply::partial(match request.axis_assignment {
                Some(_) => {
                    "replay_decoded only: 32-bit buttons and caller-assigned signed-byte sticks \
                     for stored packets; slots without a packet are missing; physical and \
                     effective input unsupported"
                }
                None => {
                    "replay_decoded only: 32-bit buttons for stored packets; slots without a \
                     packet are missing; semantic sticks unsupported without a caller axis \
                     assignment (stored axes kept raw); physical and effective input unsupported"
                }
            }),
            ..Default::default()
        },
        clocks: ClockSupport {
            supplied: vec![ClockKind::Record],
            relations: Vec::new(),
        },
        notes,
    }
}

/// The decoded-replay input of one slot in one record. Semantic sticks are
/// `unsupported` without a caller axis assignment, observed from the stored axes
/// with one, and `missing` with one when the slot has no stored packet.
pub fn slot_input(slot: &SlotRecord, axes: Option<AxisAssignment>) -> InputEvent {
    let (buttons, raw) = match &slot.packet {
        Some(packet) => (
            Channel::Observed(Buttons::new(slot.expanded_buttons)),
            Some(RawBytes::new(PACKET_ENCODING, &packet.bytes)),
        ),
        None => (Channel::Missing, None),
    };
    let (left_stick, right_stick) = match (axes, &slot.packet) {
        (None, _) => (Channel::Unsupported, Channel::Unsupported),
        (Some(axes), Some(packet)) => {
            let (left, right) = axes.sticks(packet.axes);
            (Channel::Observed(left), Channel::Observed(right))
        }
        (Some(_), None) => (Channel::Missing, Channel::Missing),
    };
    InputEvent {
        controller: Some(slot.slot),
        entry: None,
        actor: None,
        origin: InputOrigin::ReplayDecoded,
        layout: INPUT_LAYOUT_ID.to_string(),
        buttons,
        left_stick,
        right_stick,
        raw,
    }
}

/// Record every decoded record into `recorder`: one `raw` event per record, then
/// one `input` per active slot, all at `clocks.record = ordinal`. `axes` must be
/// the assignment the recorder's manifest was built with.
pub fn record_replay(
    recorder: &mut Recorder,
    decoded: &DecodedReplay,
    axes: Option<AxisAssignment>,
) -> Result<(), CaptureError> {
    let active = decoded.active_slots();
    for record in &decoded.records {
        let clocks = Clocks::record(record.ordinal as u64);
        recorder.push(
            clocks,
            EventBody::Raw(RawUnit {
                raw: RawBytes::new(RECORD_ENCODING, &record.bytes),
                source_offset: Some(record.payload_offset as u64),
            }),
        )?;
        for &slot in &active {
            recorder.input(clocks, slot_input(&record.slots[usize::from(slot)], axes))?;
        }
    }
    Ok(())
}

fn finish(
    mut recorder: Recorder,
    decoded: &DecodedReplay,
    axes: Option<AxisAssignment>,
) -> Result<Finished, ReplayImportError> {
    record_replay(&mut recorder, decoded, axes)?;
    let clocks = decoded
        .records
        .last()
        .map_or(Clocks::NONE, |record| Clocks::record(record.ordinal as u64));
    Ok(recorder.finish(clocks, Completion::complete())?)
}

/// Decode and map a saved replay into an in-memory capture.
pub fn import_replay(request: &ReplayImportRequest<'_>) -> Result<Capture, ReplayImportError> {
    let decoded = decode_replay(request)?;
    let recorder = Recorder::in_memory(capture_header(request, &decoded))?;
    let finished = finish(recorder, &decoded, request.axis_assignment)?;
    Ok(finished
        .into_capture()
        .expect("in-memory recorders keep their events"))
}

/// Decode and stream a saved replay into a capture directory. Decoding finishes
/// before the directory is created, so a rejected payload writes nothing.
pub fn import_replay_to_dir(
    request: &ReplayImportRequest<'_>,
    dir: impl AsRef<Path>,
    options: RecorderOptions,
) -> Result<Finished, ReplayImportError> {
    let decoded = decode_replay(request)?;
    let recorder = Recorder::create(dir, capture_header(request, &decoded), options)?;
    finish(recorder, &decoded, request.axis_assignment)
}
