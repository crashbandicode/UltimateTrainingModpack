//! Value-only checks for saved-original same-class live adapters.
//!
//! Inputs are owned copies, never Rust references into native-owned memory. These checks do not
//! acquire a lock, pin G/FE, establish a callback lifetime, or authorize a native write. Native glue
//! must bind the current FE and rederive holder/G/vector/index inside its qualified pool lock.

use crate::inventory::{MiiClass, CHARINFO_LEN};
use crate::lifetime::{Binding, Configuration, Tokens, Transaction};

pub const RECORD_LEN: usize = 0x68;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Scope,
    OtherMii,
    Binding,
    Vector,
    SavedRow,
    FighterMirror,
    InformationMirror,
    OwnerMirror,
    RowIdentity,
    HatChanged,
    RecordChanged,
}

/// Actual u64 results of the fingerprinted native getters, not asset field/index accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowIds {
    pub outfit: u64,
    pub hat: u64,
}

impl RowIds {
    pub fn validate(self, configuration: Configuration) -> Result<(), Error> {
        let tokens = configuration.tokens();
        let appearance = configuration.canonical().appearance;
        self.validate_tokens(tokens, appearance.outfit, appearance.hat)
    }

    pub fn validate_tokens(
        self,
        tokens: Tokens,
        outfit_hash: u64,
        hat_hash: u64,
    ) -> Result<(), Error> {
        for (tag, hint, hash, actual) in [
            (0x13u64, tokens.outfit, outfit_hash, self.outfit),
            (0x5au64, tokens.hat, hat_hash, self.hat),
        ] {
            if hint == u16::MAX
                || hash == 0
                || hash & !0xff_ffff_ffff != 0
                || actual != ((tag << 56) | (u64::from(hint) << 40) | hash)
            {
                return Err(Error::RowIdentity);
            }
        }
        Ok(())
    }
}

/// E, FE, D and O copied values. D has FOUR halfword mirrors; D+0x7a is not voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mirrors {
    pub index: u16,
    pub tokens: Tokens,
    pub fe: [u16; 5],
    pub d: [u16; 4],
    pub d_specials: [u32; 4],
    pub owner_index: u16,
    pub owner: [u32; 4],
}

impl Mirrors {
    /// Exact owned byte windows: E+1a8..1b6, FE+46..50, D+72..7a, D+390..3a0,
    /// O+68..7c. O+6a padding is ignored, not reinterpreted as a token.
    pub fn decode(
        e: [u8; 14],
        fe: [u8; 10],
        d: [u8; 8],
        d_specials: [u8; 16],
        owner: [u8; 20],
    ) -> Self {
        Self {
            index: half(&e, 0),
            tokens: Tokens {
                outfit: half(&e, 2),
                hat: half(&e, 4),
                colour: half(&e, 6),
                voice: half(&e, 8),
                specials: [e[10], e[11], e[12], e[13]],
            },
            fe: core::array::from_fn(|i| half(&fe, i * 2)),
            d: core::array::from_fn(|i| half(&d, i * 2)),
            d_specials: core::array::from_fn(|i| word(&d_specials, i * 4)),
            owner_index: half(&owner, 0),
            owner: core::array::from_fn(|i| word(&owner, 4 + i * 4)),
        }
    }

    pub fn validate(self, index: u16, expected: Tokens) -> Result<(), Error> {
        if self.index != index
            || self.tokens != expected
            || expected.specials.iter().any(|s| *s > 2)
        {
            return Err(Error::SavedRow);
        }
        if self.fe
            != [
                index,
                expected.outfit,
                expected.hat,
                expected.colour,
                expected.voice,
            ]
        {
            return Err(Error::FighterMirror);
        }
        if self.d != [index, expected.outfit, expected.hat, expected.colour]
            || self.d_specials != expected.specials.map(u32::from)
        {
            return Err(Error::InformationMirror);
        }
        if self.owner_index != index
            || self.owner
                != [
                    expected.outfit,
                    expected.hat,
                    expected.colour,
                    expected.voice,
                ]
                .map(u32::from)
        {
            return Err(Error::OwnerMirror);
        }
        Ok(())
    }
}

fn half(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Exact expected identity, including pool endpoints, not merely the old record count. The
/// returned address is a numeric value for a native adapter; it is NOT dereference authority.
pub fn record_address(expected: Binding, fresh: Binding) -> Result<usize, Error> {
    if fresh != expected {
        return Err(Error::Binding);
    }
    let aligned = |p: usize| p != 0 && p.is_multiple_of(8);
    if ![
        fresh.entry,
        fresh.row,
        fresh.holder,
        fresh.manager,
        fresh.vector_begin,
    ]
    .into_iter()
    .all(aligned)
    {
        return Err(Error::Vector);
    }
    let length = fresh
        .vector_end
        .checked_sub(fresh.vector_begin)
        .ok_or(Error::Vector)?;
    if !length.is_multiple_of(RECORD_LEN) || usize::from(fresh.record_index) >= length / RECORD_LEN
    {
        return Err(Error::Vector);
    }
    fresh
        .vector_begin
        .checked_add(usize::from(fresh.record_index) * RECORD_LEN)
        .ok_or(Error::Vector)
}

/// A freshly copied current owner map is required, including inactive native helpers. None
/// means a positively absent entry, not an unreadable or unchecked manager lookup.
pub fn qualified_scope(
    fresh: Binding,
    saved_row_kind: u32,
    owner_kinds: [Option<u32>; 8],
) -> Result<(), Error> {
    if saved_row_kind != 0x49 { return Err(Error::Scope); }
    qualified_saved_scope(fresh, saved_row_kind, owner_kinds)
}

/// Same-class saved-original scope; no ordinary-origin or second Mii admission.
pub fn qualified_saved_scope(
    fresh: Binding, saved_row_kind: u32, owner_kinds: [Option<u32>; 8],
) -> Result<(), Error> {
    if fresh.owner != 0 || MiiClass::from_kind(saved_row_kind).is_none()
        || owner_kinds[0] != Some(saved_row_kind) {
        return Err(Error::Scope);
    }
    if owner_kinds[1..]
        .iter()
        .flatten()
        .any(|kind| (0x48..=0x4a).contains(kind))
    {
        return Err(Error::OtherMii);
    }
    Ok(())
}

/// Copied current associations for the narrow CPU diagnostic. Addresses are identities only.
/// This is not native dereference authority or production Mii admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuOwner {
    pub owner: u32,
    pub kind: u32,
    pub selector: u8,
    pub index: u16,
    pub entry: usize,
    pub row: usize,
    pub information: usize,
    pub object: usize,
    pub alias: u8,
}

/// Origin01 #284/#297/#298: saved Sword CPU1 and native inactive Swords2/3,
/// distinct current objects and indices. P1 is the explicitly qualified ordinary Luigi fixture.
/// Ordinary P1's zero Mii index is observed, not treated as a claim on record0.
pub fn qualified_cpu_scope(
    owners: &[CpuOwner; 4],
    other_entries_present: bool,
) -> Result<(), Error> {
    qualified_saved_cpu_scope(owners, other_entries_present, 0x49)
}

/// Retains exact owner/index/alias uniqueness and Luigi P1; all three CPU/helper kinds
/// must equal the captured saved class. Inactive helpers are never available records.
pub fn qualified_saved_cpu_scope(
    owners: &[CpuOwner; 4], other_entries_present: bool, kind: u32,
) -> Result<(), Error> {
    if MiiClass::from_kind(kind).is_none() || other_entries_present {
        return Err(Error::Scope);
    }
    for (id, owner) in owners.iter().enumerate() {
        if owner.owner != id as u32
            || owner.selector != id as u8
            || owner.kind != if id == 0 { 9 } else { kind }
            || (id != 0 && owner.index != id as u16)
            || owner.alias != 0
            || [owner.entry, owner.row, owner.information, owner.object]
                .iter()
                .any(|p| *p == 0 || !p.is_multiple_of(8))
        {
            return Err(Error::Scope);
        }
        for previous in &owners[..id] {
            if owner.entry == previous.entry
                || owner.row == previous.row
                || owner.information == previous.information
                || owner.object == previous.object
            {
                return Err(Error::Binding);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuHelper {
    pub owner: CpuOwner,
    pub mirrors: Mirrors,
    pub record: [u8; RECORD_LEN],
    pub body: usize,
    pub body_id: u32,
}

/// Helper configuration and numeric ownership are invariant across the target's reload.
/// Shared-pointer words are compared as bytes, never dereferenced or reference-counted here.
pub fn preserved_cpu_helpers(
    expected: &[CpuHelper; 2],
    current: &[CpuHelper; 2],
) -> Result<(), Error> {
    if expected != current {
        return Err(Error::RecordChanged);
    }
    Ok(())
}

/// An owned update payload. The only mutation method below modifies an owned record COPY.
/// In particular, it cannot lock G, write native tokens, call INSTALL or complete a transaction.
#[derive(Clone, Copy)]
pub struct RecordUpdate {
    before: [u8; CHARINFO_LEN],
    after: [u8; CHARINFO_LEN],
    tokens: Tokens,
}

impl RecordUpdate {
    /// Called with freshly copied values from the qualified critical section. All validation
    /// precedes publication. Native code must perform the same checks on its current identities
    /// and exact record immediately before copying 0x58 bytes, while still holding that lock.
    pub fn prepare(
        transaction: Transaction,
        fresh: Binding,
        mirrors: Mirrors,
        rows: [RowIds; 2],
        record: &[u8; RECORD_LEN],
    ) -> Result<Self, Error> {
        if transaction.before().canonical().class != MiiClass::Swordfighter {
            return Err(Error::Scope);
        }
        Self::prepare_for_role(crate::selection::Role::Player, transaction, fresh, mirrors, rows, record)
    }

    /// Value kernel for the role adapter after its complete scope/preservation validation.
    /// The public legacy entry point above retains its original P1-only scope.
    pub(crate) fn prepare_for_role(
        role: crate::selection::Role,
        transaction: Transaction,
        fresh: Binding,
        mirrors: Mirrors,
        rows: [RowIds; 2],
        record: &[u8; RECORD_LEN],
    ) -> Result<Self, Error> {
        let before = transaction.before();
        let after = transaction.after();
        let owner = match role { crate::selection::Role::Player => 0, crate::selection::Role::Cpu => 1 };
        if fresh.owner != owner || fresh.record_index != owner as u16
            || before.canonical().class != after.canonical().class
        {
            return Err(Error::Scope);
        }
        record_address(transaction.binding(), fresh)?;
        mirrors.validate(fresh.record_index, before.tokens())?;
        rows[0].validate(before)?;
        rows[1].validate(after)?;
        if before.tokens().hat != after.tokens().hat
            || before.canonical().appearance.hat != after.canonical().appearance.hat
        {
            return Err(Error::HatChanged);
        }
        Self::compare_record(
            *before.charinfo(),
            *after.charinfo(),
            after.tokens(),
            record,
        )
    }

    /// Exact copied-record kernel shared by the transaction adapter and the existing diagnostic.
    /// This alone does not establish scope, row identity, mirrors, a lock or native write authority.
    pub fn compare_record(
        before: [u8; CHARINFO_LEN],
        after: [u8; CHARINFO_LEN],
        tokens: Tokens,
        record: &[u8; RECORD_LEN],
    ) -> Result<Self, Error> {
        if record[..CHARINFO_LEN] != before {
            return Err(Error::RecordChanged);
        }
        Ok(Self {
            before,
            after,
            tokens,
        })
    }

    pub fn tokens(self) -> Tokens {
        self.tokens
    }
    pub fn charinfo(&self) -> &[u8; CHARINFO_LEN] {
        &self.after
    }

    /// Fault-atomic value operation for host validation. Never create this mutable Rust reference
    /// from concurrent native memory. The shared_ptr data/control bytes +58..68 are untouched.
    pub fn apply_to_copy(self, record: &mut [u8; RECORD_LEN]) -> Result<(), Error> {
        if record[..CHARINFO_LEN] != self.before {
            return Err(Error::RecordChanged);
        }
        record[..CHARINFO_LEN].copy_from_slice(&self.after);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_sword_mirrors_use_four_d_halfwords_and_full_width_owner_tokens() {
        // Original saved Sword: index0, outfit41, hat0, colour5, voice37, 2/6/2/2 -> 0/2/0/0.
        let values = Mirrors::decode(
            [0, 0, 41, 0, 0, 0, 5, 0, 37, 0, 0, 2, 0, 0],
            [0, 0, 41, 0, 0, 0, 5, 0, 37, 0],
            [0, 0, 41, 0, 0, 0, 5, 0],
            [0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [
                0, 0, 0xfe, 0xed, 41, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 37, 0, 0, 0,
            ],
        );
        let expected = Tokens {
            outfit: 41,
            hat: 0,
            colour: 5,
            voice: 37,
            specials: [0, 2, 0, 0],
        };
        assert_eq!(values.validate(0, expected), Ok(()));
        let mut wrong = values;
        wrong.owner[3] = 0x10025; // low halfword agrees, native u32 does not
        assert_eq!(wrong.validate(0, expected), Err(Error::OwnerMirror));
        wrong = values;
        wrong.d_specials[1] = 0x102;
        assert_eq!(wrong.validate(0, expected), Err(Error::InformationMirror));
        wrong = values;
        wrong.owner_index = 1;
        assert_eq!(wrong.validate(0, expected), Err(Error::OwnerMirror));
    }

    fn binding() -> Binding {
        Binding {
            epoch: 5,
            owner: 0,
            entry: 0x1000,
            row: 0x2000,
            holder: 0x3000,
            manager: 0x4000,
            vector_begin: 0x5000,
            vector_end: 0x5000 + 133 * RECORD_LEN,
            record_index: 0,
        }
    }

    #[test]
    fn unchanged_count_does_not_hide_pool_relocation_or_owner_alias() {
        let original = binding();
        assert_eq!(record_address(original, original), Ok(0x5000));
        let mut moved = original;
        moved.vector_begin += RECORD_LEN;
        moved.vector_end += RECORD_LEN;
        assert_eq!(record_address(original, moved), Err(Error::Binding));
        moved = original;
        moved.manager += 8;
        assert_eq!(record_address(original, moved), Err(Error::Binding));
        let mut kinds = [None; 8];
        kinds[0] = Some(0x49);
        kinds[1] = Some(9);
        assert_eq!(qualified_scope(original, 0x49, kinds), Ok(()));
        kinds[3] = Some(0x49); // inactive helper is still another Mii owner
        assert_eq!(qualified_scope(original, 0x49, kinds), Err(Error::OtherMii));
    }

    fn captured_cpu_owners() -> [CpuOwner; 4] {
        // Origin01 first visit: actual #281/#284/#297/#298 FE/D/O addresses.
        // E identities follow the converter captures and their native 0x260 stride.
        let entries = [0x120293e000, 0x1202944000, 0x120294a000, 0x12053e0000];
        let objects = [0x11b0867900, 0x11f3e48000, 0x11f3e28700, 0x11f3e43a00];
        core::array::from_fn(|id| CpuOwner {
            owner: id as u32,
            kind: if id == 0 { 9 } else { 0x49 },
            selector: id as u8,
            index: id as u16,
            entry: entries[id],
            row: 0x8531d730 + id * 0x260,
            information: entries[id] + 0x110,
            object: objects[id],
            alias: 0,
        })
    }

    #[test]
    fn saved_cpu_assignments_do_not_admit_index_or_owner_aliases_or_widen_p1() {
        let owners = captured_cpu_owners();
        assert_eq!(qualified_cpu_scope(&owners, false), Ok(()));
        let mut aliased = owners;
        aliased[2].index = 1; // same CharInfo is not permission to reuse CPU1's record
        assert_eq!(qualified_cpu_scope(&aliased, false), Err(Error::Scope));
        aliased = owners;
        aliased[1].object = owners[0].object;
        assert_eq!(qualified_cpu_scope(&aliased, false), Err(Error::Binding));
        aliased = owners;
        aliased[1].alias = 1;
        assert_eq!(qualified_cpu_scope(&aliased, false), Err(Error::Scope));
        aliased = owners;
        aliased[1].selector = 0;
        assert_eq!(qualified_cpu_scope(&aliased, false), Err(Error::Scope));
        assert_eq!(qualified_cpu_scope(&owners, true), Err(Error::Scope));
        let p1_kinds = [
            Some(0x49),
            Some(9),
            Some(0x49),
            Some(0x49),
            None,
            None,
            None,
            None,
        ];
        assert_eq!(
            qualified_scope(binding(), 0x49, p1_kinds),
            Err(Error::OtherMii)
        );
    }

    #[test]
    fn same_helper_hash_does_not_hide_record_resource_or_mirror_changes() {
        let owners = captured_cpu_owners();
        let expected = core::array::from_fn(|i| {
            let index = (i + 2) as u16;
            CpuHelper {
                owner: owners[i + 2],
                mirrors: Mirrors {
                    index,
                    tokens: Tokens {
                        outfit: 41,
                        hat: 0,
                        colour: 5,
                        voice: 37,
                        specials: [0, 0, 0, 0],
                    },
                    fe: [index, 41, 0, 5, 37],
                    d: [index, 41, 0, 5],
                    d_specials: [0, 0, 0, 0],
                    owner_index: index,
                    owner: [41, 0, 5, 37],
                },
                // Synthetic record bytes; native token/owner values above are captured.
                record: [0x5a; RECORD_LEN],
                body: 0x1000 + i * 0x1000,
                body_id: i as u32 + 2,
            }
        });
        assert_eq!(preserved_cpu_helpers(&expected, &expected), Ok(()));
        let mut changed = expected;
        changed[0].record[CHARINFO_LEN] ^= 1; // unchanged CharInfo, different shared_ptr data
        assert_eq!(
            preserved_cpu_helpers(&expected, &changed),
            Err(Error::RecordChanged)
        );
        changed = expected;
        changed[1].record[CHARINFO_LEN + 8] ^= 1; // control block is also part of preservation
        assert_eq!(
            preserved_cpu_helpers(&expected, &changed),
            Err(Error::RecordChanged)
        );
        changed = expected;
        changed[1].mirrors.owner_index = 1;
        assert_eq!(
            preserved_cpu_helpers(&expected, &changed),
            Err(Error::RecordChanged)
        );
    }

    #[test]
    fn record_copy_compare_is_exact_and_preserves_shared_ownership_bytes() {
        let before = [0x11; CHARINFO_LEN];
        let after = [0x22; CHARINFO_LEN];
        let target = Tokens {
            outfit: 43,
            hat: 0,
            colour: 4,
            voice: 37,
            specials: [1, 2, 0, 0],
        };
        let mut record = [0xa5; RECORD_LEN];
        record[..CHARINFO_LEN].copy_from_slice(&before);
        let update = RecordUpdate::compare_record(before, after, target, &record).unwrap();
        let mut changed = record;
        changed[0] ^= 1; // catalog masks this byte, exact live compare must not
        let saved = changed;
        assert!(matches!(
            RecordUpdate::compare_record(before, after, target, &changed),
            Err(Error::RecordChanged)
        ));
        assert_eq!(
            update.apply_to_copy(&mut changed),
            Err(Error::RecordChanged)
        );
        assert_eq!(changed, saved);
        assert_eq!(update.apply_to_copy(&mut record), Ok(()));
        assert_eq!(&record[..CHARINFO_LEN], &after);
        assert_eq!(&record[CHARINFO_LEN..], &[0xa5; 16]);
    }

    #[test]
    fn native_tagged_rows_require_saved_hint_and_hash_not_an_asset_index() {
        let tokens = Tokens {
            outfit: 43,
            hat: 0,
            colour: 4,
            voice: 37,
            specials: [1, 2, 0, 0],
        };
        let rows = RowIds {
            outfit: 0x13002b147bc59435,
            hat: 0x5a00000fa9efa5ea,
        };
        assert_eq!(
            rows.validate_tokens(tokens, 0x147bc59435, 0x0fa9efa5ea),
            Ok(())
        );
        let wrong_hint = Tokens {
            outfit: 42,
            ..tokens
        };
        assert_eq!(
            rows.validate_tokens(wrong_hint, 0x147bc59435, 0x0fa9efa5ea),
            Err(Error::RowIdentity)
        );
        assert_eq!(
            RowIds { outfit: 43, ..rows }.validate_tokens(tokens, 0x147bc59435, 0x0fa9efa5ea),
            Err(Error::RowIdentity)
        );
    }
}
