//! Native entry descriptors and form indices. Values only; callers own entry lifetime.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forms {
    pub count: u32,
    pub kinds: [u32; 3],
    pub actor_count: u32,
    pub flags: u8,
}

impl Forms {
    /// Matches the native descriptor expansion. The caller admits catalog kinds.
    pub fn starting(kind: u32) -> Self {
        let (count, kinds, flags) = match kind {
            0x24 => (3, [0x24, 0x25, 0x26], 0x05),
            0x25 => (3, [0x25, 0x26, 0x24], 0x05),
            0x26 => (3, [0x26, 0x24, 0x25], 0x05),
            0x5a => (2, [0x5a, 0x5b, u32::MAX], 0x64),
            0x5b => (2, [0x5b, 0x5a, u32::MAX], 0x64),
            0x4b => (2, [0x4b, 0x4c, u32::MAX], 0x02),
            _ => (1, [kind, u32::MAX, u32::MAX], 0),
        };
        Self {
            count,
            kinds,
            actor_count: if kind == 0x4b { 2 } else { 1 },
            flags,
        }
    }

    pub fn solo_popo() -> Self {
        Self {
            count: 1,
            kinds: [0x4b, u32::MAX, u32::MAX],
            actor_count: 1,
            flags: 0,
        }
    }

    /// Simultaneous partners stay active; transformation alternatives sleep.
    pub fn body_state(self, index: usize, active: u8) -> u8 {
        if self.actor_count == self.count || index == usize::from(active) {
            4
        } else {
            3
        }
    }

    pub fn index_of(self, kind: u32) -> Option<u8> {
        self.kinds[..self.count as usize]
            .iter()
            .position(|k| *k == kind)
            .map(|i| i as u8)
    }

    pub fn same_group(self, kind: u32) -> bool {
        self.index_of(kind).is_some()
    }
}

/// Native resource members belong to the requested source's canonical group.
/// Partners need not be separate selectable choices; the request still must be.
pub fn is_source_member(requested: u32, member: u32) -> bool {
    if (0x48..=0x4a).contains(&requested) {
        // A Mii source has exactly its own class member. Generic archive,
        // typed-param and copy-file readiness remain mandatory at the caller.
        return cfg!(feature = "mii-managed-helper-alpha") && requested == member;
    }
    crate::TrioFighter::from_kind(requested).is_some()
        && Forms::starting(requested).same_group(member)
}

/// FighterParamAccessor2 record key for a validated native source member.
/// Native acquire (main+607b34) and release (main+720ea4) map Nana to Popo.
/// This does not change descriptor, body, NRO, archive or copy-resource kinds.
pub fn typed_record_kind(requested: u32, member: u32) -> Option<u32> {
    if !is_source_member(requested, member) {
        return None;
    }
    Some(if member == 0x4c { 0x4b } else { member })
}

#[derive(Clone, Copy, Debug)]
pub struct EntryState {
    pub count: u32,
    pub kinds: [u32; 3],
    pub actor_count: u32,
    pub installed: u64,
    pub index: u8,
    pub previous_or_next: u8,
    pub pending: u8,
    pub flags: u8,
    pub outer: u8,
    pub loader: u8,
    pub changing: u8,
    pub guard: u8,
}

impl EntryState {
    /// Initial loads use ff in +5919; completed form changes use the live index.
    /// Pointer/owner/body checks remain the native callback's responsibility.
    pub fn ready(self) -> Option<(Forms, u32)> {
        let forms = if self.kinds[0] == 0x4b && self.count == 1 {
            Forms::solo_popo()
        } else {
            Forms::starting(self.kinds[0])
        };
        (self.count == forms.count
            && self.kinds == forms.kinds
            && self.actor_count == forms.actor_count
            && self.installed == u64::from(forms.count)
            && u32::from(self.index) < forms.count
            && (self.previous_or_next == 0xff || self.previous_or_next == self.index)
            && self.pending == 0xff
            && self.flags == forms.flags
            && self.outer == 6
            && self.loader == 14
            && self.changing == 0
            && self.guard == 0)
            .then(|| (forms, self.kinds[self.index as usize]))
    }
}

/// Ice Climbers body for a slot. Player slot 0 of an Ice Climbers original (`numbered`
/// false, which includes a direct pick) follows the original-select preference; every
/// numbered slot, and a direct pick over another original, follows the cycle's. CPU has
/// one preference for its original and its cycle.
pub fn configured_forms(
    menu: &crate::TrainingModpackMenu,
    cpu: bool,
    original: Option<u32>,
    kind: u32,
    numbered: bool,
) -> Forms {
    let solo = if cpu {
        menu.solo_popo_cpu
    } else if !numbered && original == Some(0x4b) {
        menu.solo_popo_original
    } else {
        menu.solo_popo_player_cycle
    };
    if kind == 0x4b && solo == crate::OnOff::ON {
        Forms::solo_popo()
    } else {
        Forms::starting(kind)
    }
}

/// Zero-based numbered slot holding `current`'s native group, if any.
pub fn cycle_position(choices: &[u32], current: u32) -> Option<usize> {
    choices
        .iter()
        .position(|kind| Forms::starting(*kind).same_group(current))
}

/// The step after Player slot 0. `next_cycle` finds a live fighter by group, so it cannot
/// tell slot 0 from a numbered slot of the same group (Ice Climbers, when its Solo Popo
/// preference differs between the two); the caller knows and asks for this instead.
pub fn next_from_original(
    choices: &[u32],
    original: Option<u32>,
) -> Option<(u32, Option<usize>)> {
    let original = catalog_original(original);
    choices
        .first()
        .map(|kind| (*kind, Some(0)))
        .or_else(|| original.map(|kind| (kind, None)))
}

/// Slot 0 is a catalog fighter. `u32::MAX` and any other non-catalog kind are
/// missing, not a selected identity.
fn catalog_original(original: Option<u32>) -> Option<u32> {
    original.filter(|kind| crate::TrioFighter::from_kind(*kind).is_some())
}

/// Ordered slots are zero-based internally; None is the original CSS slot 0.
/// A live transformed partner belongs to its group's existing numbered slot.
pub fn next_cycle(
    current: u32,
    cursor: Option<usize>,
    choices: &[u32],
    original: Option<u32>,
) -> Option<(u32, Option<usize>)> {
    let original = catalog_original(original);
    let position = cursor
        .filter(|i| *i < choices.len() && Forms::starting(choices[*i]).same_group(current))
        .or_else(|| cycle_position(choices, current));
    if let Some(index) = position {
        if index + 1 < choices.len() {
            return Some((choices[index + 1], Some(index + 1)));
        }
        if let Some(kind) = original {
            return Some((kind, None));
        }
    }
    next_from_original(choices, original)
}

/// Entry identity, owner and kind of a settled practice body. The identity is
/// compared only, never dereferenced.
pub type Receipt = (usize, u32, u32);

/// What a native presentation rebuild (Training reset) does for the live body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rebuild {
    /// Native reset presents the live ordinary fighter itself.
    Admit(Receipt),
    /// Native reset keeps its own rebuild. The next service callback republishes
    /// this Mii publication, and the exact-portrait lifecycle restores its texture.
    Republish(Receipt),
    /// Not a plugin publication: native reset rebuilds the original CSS portrait.
    Native,
}

fn mii_class(kind: u32) -> bool {
    (0x48..=0x4a).contains(&kind)
}

/// Native reset rebuilds the original CSS portrait unless the plugin restores the live
/// body. The ordinary loader's `loaded` receipt also gates saved-CPU idleness, so an
/// accepted Mii-adapter publication (named or replay fighter switch, saved or exact
/// Mii) never writes it. This presentation-only receipt records that publication
/// instead. An ordinary fighter is admitted into native reset. A Mii class body is
/// never admitted, because its bust belongs to the exact-portrait lifecycle; it is
/// republished after native reset instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentedReceipt(Option<Receipt>);

impl PresentedReceipt {
    pub const NONE: Self = Self(None);

    /// An accepted adapter publication replaces the previous receipt.
    pub fn publish(&mut self, entry: usize, owner: u32, kind: u32) {
        self.0 = Some((entry, owner, kind));
    }

    /// A newly observed ready kind. The same entry and owner carry the receipt to
    /// it (a native transformation); a different identity retires it.
    pub fn observe(&mut self, entry: usize, owner: u32, kind: u32) {
        if self.0.is_some_and(|(identity, presented, _)| identity == entry && presented == owner) {
            self.publish(entry, owner, kind);
        } else {
            self.0 = None;
        }
    }

    /// Scene change, or an ordinary loader replacement of the body.
    pub fn retire(&mut self) {
        self.0 = None;
    }

    /// Before that entry's native destruction.
    pub fn retire_entry(&mut self, entry: usize) {
        if self.0.is_some_and(|(identity, _, _)| identity == entry) {
            self.0 = None;
        }
    }

    /// The receipt native reset may admit for `entry` presenting `kind`. A matching
    /// loader receipt wins; a stale one cannot hide a later adapter publication,
    /// which never updates `loaded`. A Mii class publication is never admitted.
    /// The caller still verifies the live body.
    pub fn admit(self, loaded: Option<Receipt>, entry: usize, kind: u32) -> Option<Receipt> {
        let matches = move |receipt: &Receipt| receipt.0 == entry && receipt.2 == kind;
        loaded
            .filter(matches)
            .or_else(|| self.0.filter(|receipt| matches(receipt) && !mii_class(receipt.2)))
    }

    /// The single native-rebuild decision for `entry` presenting `kind`, from both
    /// native reset hooks. The caller verifies the live body before acting on it.
    pub fn rebuild(self, loaded: Option<Receipt>, entry: usize, kind: u32) -> Rebuild {
        if let Some(receipt) = self.admit(loaded, entry, kind) {
            return Rebuild::Admit(receipt);
        }
        match self.0 {
            Some(receipt) if receipt.0 == entry && receipt.2 == kind && mii_class(kind) => {
                Rebuild::Republish(receipt)
            }
            _ => Rebuild::Native,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_members_include_native_partners_without_making_them_selectable() {
        // A native Ice Climbers source owns both Popo and Nana resources.
        assert!(is_source_member(0x4b, 0x4b));
        assert!(is_source_member(0x4b, 0x4c));
        assert!(crate::TrioFighter::from_kind(0x4c).is_none());
        // Membership is scoped to a valid requested source, not a kind whitelist.
        assert!(!is_source_member(0x00, 0x4c));
        assert!(!is_source_member(0x4b, u32::MAX));
        assert!(!is_source_member(0x4c, 0x4c));
        assert_eq!(is_source_member(0x48, 0x48), cfg!(feature = "mii-managed-helper-alpha"));
        assert!(is_source_member(0x5a, 0x5b));
        assert!(is_source_member(0x24, 0x26));

        // Native acquire/release share one typed record for the two IC bodies.
        assert_eq!(typed_record_kind(0x4b, 0x4b), Some(0x4b));
        assert_eq!(typed_record_kind(0x4b, 0x4c), Some(0x4b));
        assert_eq!(Forms::starting(0x4b).kinds, [0x4b, 0x4c, u32::MAX]);
        // Mapping cannot admit an unrelated member or a nonselectable request.
        for (requested, member) in [
            (0x00, 0x4c),
            (0x4c, 0x4c),
            (0x4b, u32::MAX),
        ] {
            assert_eq!(typed_record_kind(requested, member), None);
        }
        // Ordinary and transforming members retain their own parameter records.
        for (requested, member) in [
            (0x00, 0x00),
            (0x24, 0x26),
            (0x25, 0x24),
            (0x26, 0x25),
            (0x5a, 0x5b),
            (0x5b, 0x5a),
        ] {
            assert_eq!(typed_record_kind(requested, member), Some(member));
        }
    }

    #[test]
    fn mii_source_members_keep_feature_gated_exact_typed_identity() {
        for class in 0x48..=0x4a {
            let expected = cfg!(feature = "mii-managed-helper-alpha").then_some(class);
            assert_eq!(typed_record_kind(class, class), expected);
            assert_eq!(Forms::starting(class).kinds, [class, u32::MAX, u32::MAX]);
            for other in [0x00, 0x48, 0x49, 0x4a, 0x4c, u32::MAX] {
                if other != class {
                    assert!(!is_source_member(class, other));
                    assert_eq!(typed_record_kind(class, other), None);
                }
            }
            assert_eq!(typed_record_kind(0x00, class), None);
        }
    }

    // Facts captured from unmodified native CSS loads and Down+B transformations.
    fn pyra_loaded() -> EntryState {
        EntryState {
            count: 2,
            kinds: [0x5a, 0x5b, u32::MAX],
            actor_count: 1,
            installed: 2,
            index: 0,
            previous_or_next: 0xff,
            pending: 0xff,
            flags: 0x64,
            outer: 6,
            loader: 14,
            changing: 0,
            guard: 0,
        }
    }
    #[test]
    fn native_initial_and_transformed_aegis_are_ready() {
        let mut state = pyra_loaded();
        assert_eq!(state.ready().unwrap().1, 0x5a);
        state.index = 1;
        state.previous_or_next = 1;
        assert_eq!(state.ready().unwrap().1, 0x5b);
    }
    #[test]
    fn native_ice_climbers_keep_both_bodies_active() {
        // Captured native CSS load on 13.0.5, separate from requested descriptors.
        let state = EntryState {
            count: 2,
            kinds: [0x4b, 0x4c, u32::MAX],
            actor_count: 2,
            installed: 2,
            index: 0,
            previous_or_next: 0xff,
            pending: 0xff,
            flags: 2,
            outer: 6,
            loader: 14,
            changing: 0,
            guard: 0,
        };
        let (forms, kind) = state.ready().unwrap();
        assert_eq!(kind, 0x4b);
        assert_eq!([forms.body_state(0, 0), forms.body_state(1, 0)], [4, 4]);
        assert!(EntryState {
            actor_count: 1,
            ..state
        }
        .ready()
        .is_none());
        assert!(EntryState {
            installed: 1,
            ..state
        }
        .ready()
        .is_none());
        assert_eq!(Forms::starting(0x5a).body_state(1, 0), 3);
    }
    #[test]
    fn player_visits_original_zero_and_cpu_returns_when_original_is_known() {
        let selected = [0x0b, 0x5b, 0x25];
        assert_eq!(
            next_cycle(6, None, &selected, Some(6)),
            Some((0x0b, Some(0)))
        );
        assert_eq!(
            next_cycle(0x0b, Some(0), &selected, Some(6)),
            Some((0x5b, Some(1)))
        );
        // Pyra transformed from selected Mythra and Charizard from Ivysaur.
        assert_eq!(
            next_cycle(0x5a, Some(1), &selected, Some(6)),
            Some((0x25, Some(2)))
        );
        assert_eq!(
            next_cycle(0x26, Some(2), &selected, Some(6)),
            Some((6, None))
        );
        assert_eq!(
            next_cycle(0x26, Some(2), &selected, None),
            Some((0x0b, Some(0)))
        );
        // A known CPU original is the return between loops, not another numbered copy.
        assert_eq!(
            next_cycle(0x26, Some(2), &selected, Some(0x0a)),
            Some((0x0a, None))
        );
        assert_eq!(
            next_cycle(0x0a, None, &selected, Some(0x0a)),
            Some((0x0b, Some(0)))
        );
        // Missing original does not become Mario, Ness, or a Mii class.
        assert_eq!(next_cycle(0x0a, None, &selected, None), Some((0x0b, Some(0))));
        assert_eq!(next_cycle(0x26, Some(2), &selected, Some(u32::MAX)), Some((0x0b, Some(0))));
        // Edited/reordered menu invalidates a stale cursor; actual group wins.
        assert_eq!(
            next_cycle(0x5a, Some(99), &selected, Some(6)),
            Some((0x25, Some(2)))
        );
        assert_eq!(next_cycle(6, None, &[], Some(6)), Some((6, None)));
        assert_eq!(next_cycle(6, None, &[], None), None);
    }
    #[test]
    fn native_pokemon_forms_keep_descriptor_order() {
        let mut state = EntryState {
            count: 3,
            kinds: [0x24, 0x25, 0x26],
            installed: 3,
            flags: 5,
            ..pyra_loaded()
        };
        assert_eq!(state.ready().unwrap().1, 0x24);
        state.index = 1;
        state.previous_or_next = 1;
        assert_eq!(state.ready().unwrap().1, 0x25);
        state.index = 2;
        state.previous_or_next = 2;
        assert_eq!(state.ready().unwrap().1, 0x26);
    }
    #[test]
    fn incomplete_or_mixed_descriptors_are_rejected() {
        let original = pyra_loaded();
        for bad in [
            EntryState {
                count: 1,
                ..original
            },
            EntryState {
                installed: 1,
                ..original
            },
            EntryState {
                actor_count: 2,
                ..original
            },
            EntryState {
                index: 2,
                ..original
            },
            EntryState {
                previous_or_next: 1,
                ..original
            },
            EntryState {
                pending: 1,
                ..original
            },
            EntryState {
                flags: 0,
                ..original
            },
            EntryState {
                outer: 3,
                ..original
            },
            EntryState {
                loader: 12,
                ..original
            },
            EntryState {
                changing: 1,
                ..original
            },
            EntryState {
                guard: 1,
                ..original
            },
            EntryState {
                kinds: [0x5a, 0x25, u32::MAX],
                ..original
            },
        ] {
            assert!(bad.ready().is_none(), "{:?}", bad);
        }
    }
    #[test]
    fn requested_first_form_has_a_complete_canonical_group() {
        // Independent static native expansion table, including non-first starts.
        for (kind, count, kinds) in [
            (0x06, 1, [0x06, u32::MAX, u32::MAX]),
            (0x5b, 2, [0x5b, 0x5a, u32::MAX]),
            (0x25, 3, [0x25, 0x26, 0x24]),
            (0x26, 3, [0x26, 0x24, 0x25]),
        ] {
            let forms = Forms::starting(kind);
            assert_eq!((forms.count, forms.kinds), (count, kinds));
            assert_eq!(forms.index_of(kind), Some(0));
            assert!(!forms.same_group(0x0b));
        }
        assert_eq!(Forms::starting(0x25).index_of(0x24), Some(2));
        assert_eq!(Forms::starting(0x26).index_of(0x25), Some(2));
    }

    #[test]
    fn solo_preference_follows_the_slot_not_just_the_kind() {
        let (off, on) = (crate::OnOff::OFF, crate::OnOff::ON);
        const IC: u32 = 0x4b;
        let menu = |original, cycle, cpu| {
            let mut saved = crate::BASE_MENU;
            saved.solo_popo_original = original;
            saved.solo_popo_player_cycle = cycle;
            saved.solo_popo_cpu = cpu;
            saved
        };
        let (solo, pair) = (Forms::solo_popo(), Forms::starting(IC));
        // Solo original, ordinary cycle: slot 0 and a direct pick are Popo alone, numbered are not.
        let m = menu(on, off, off);
        assert_eq!(configured_forms(&m, false, Some(IC), IC, false), solo);
        assert_eq!(configured_forms(&m, false, Some(IC), IC, true), pair);
        // Ordinary original, Solo cycle: the reverse.
        let m = menu(off, on, off);
        assert_eq!(configured_forms(&m, false, Some(IC), IC, false), pair);
        assert_eq!(configured_forms(&m, false, Some(IC), IC, true), solo);
        // Another original has no slot-0 Ice Climbers: its cycle preference covers a direct pick too.
        for numbered in [false, true] {
            assert_eq!(configured_forms(&m, false, Some(0x00), IC, numbered), solo);
            assert_eq!(configured_forms(&m, false, None, IC, numbered), solo);
            let m = menu(on, off, on);
            assert_eq!(configured_forms(&m, false, Some(0x00), IC, numbered), pair);
        }
        // CPU reads its one preference for the original and every numbered slot.
        for (cpu, expected) in [(on, solo), (off, pair)] {
            let m = menu(cpu_flip(cpu), cpu_flip(cpu), cpu);
            for numbered in [false, true] {
                assert_eq!(configured_forms(&m, true, Some(IC), IC, numbered), expected);
            }
        }
        // Only Ice Climbers is ever solo.
        let m = menu(on, on, on);
        for kind in [0x00, 0x24, 0x5a] {
            for cpu in [false, true] {
                assert_eq!(configured_forms(&m, cpu, Some(IC), kind, false), Forms::starting(kind));
            }
        }
    }

    fn cpu_flip(value: crate::OnOff) -> crate::OnOff {
        if value == crate::OnOff::ON { crate::OnOff::OFF } else { crate::OnOff::ON }
    }

    #[test]
    fn original_ice_climbers_and_a_numbered_ice_climbers_are_distinct_cycle_positions() {
        const IC: u32 = 0x4b;
        // Original Ice Climbers whose Solo preference differs from the cycle's: IC is also numbered slot 2.
        let choices = [0x00, 0x0b, IC, 0x5d];
        assert_eq!(cycle_position(&choices, IC), Some(2));
        assert_eq!(cycle_position(&choices, 0x5a), None);
        // Slot 0 walks into the numbered order from its first entry, not from the numbered Ice Climbers.
        assert_eq!(next_from_original(&choices, Some(IC)), Some((0x00, Some(0))));
        assert_eq!(next_cycle(0x00, Some(0), &choices, Some(IC)), Some((0x0b, Some(1))));
        assert_eq!(next_cycle(0x0b, Some(1), &choices, Some(IC)), Some((IC, Some(2))));
        assert_eq!(next_cycle(IC, Some(2), &choices, Some(IC)), Some((0x5d, Some(3))));
        assert_eq!(next_cycle(0x5d, Some(3), &choices, Some(IC)), Some((IC, None)));
        // The live numbered Ice Climbers keeps its place even if its hint was lost to a roster edit,
        // while the caller-known slot 0 does not get mistaken for it.
        assert_eq!(next_cycle(IC, None, &choices, Some(IC)), Some((0x5d, Some(3))));
        assert_eq!(next_from_original(&choices, Some(IC)), Some((0x00, Some(0))));
        // Ice Climbers alone: slot 0 steps into its numbered twin and that returns to slot 0.
        assert_eq!(next_from_original(&[IC], Some(IC)), Some((IC, Some(0))));
        assert_eq!(next_cycle(IC, Some(0), &[IC], Some(IC)), Some((IC, None)));
        // No numbered choices at all leaves only the original.
        assert_eq!(next_from_original(&[], Some(IC)), Some((IC, None)));
        assert_eq!(next_from_original(&[], None), None);
    }

    // Training CPU entry identity from a console log; compared only.
    const CPU_ENTRY: usize = 0x11aed3e000;
    const OTHER_ENTRY: usize = 0x11aed44000;
    const FOX: u32 = 0x07;
    const ROSALINA: u32 = 0x33;

    #[test]
    fn adapter_switch_from_css_rosalina_to_fox_is_admitted_without_a_loader_receipt() {
        let mut receipt = PresentedReceipt::NONE;
        // CSS Rosalina: nothing was published; native reset keeps its own path.
        assert_eq!(receipt.admit(None, CPU_ENTRY, ROSALINA), None);
        // A named or replay Switch publishes Fox through the Mii adapter and leaves
        // `loaded` empty so saved-CPU idleness is unchanged.
        receipt.publish(CPU_ENTRY, 1, FOX);
        assert_eq!(receipt.admit(None, CPU_ENTRY, FOX), Some((CPU_ENTRY, 1, FOX)));
        // The receipt cannot admit another live kind or another entry.
        assert_eq!(receipt.admit(None, CPU_ENTRY, ROSALINA), None);
        assert_eq!(receipt.admit(None, OTHER_ENTRY, FOX), None);
    }

    #[test]
    fn presented_receipt_retires_with_its_scene_entry_identity_and_loader_replacement() {
        let mut receipt = PresentedReceipt::NONE;
        receipt.publish(CPU_ENTRY, 1, FOX);
        // Another entry's destruction keeps it; its own destruction retires it.
        receipt.retire_entry(OTHER_ENTRY);
        assert_eq!(receipt.admit(None, CPU_ENTRY, FOX), Some((CPU_ENTRY, 1, FOX)));
        receipt.retire_entry(CPU_ENTRY);
        assert_eq!(receipt, PresentedReceipt::NONE);
        // Scene change or an ordinary loader install.
        receipt.publish(CPU_ENTRY, 1, FOX);
        receipt.retire();
        assert_eq!(receipt.admit(None, CPU_ENTRY, FOX), None);
        // A new entry identity, or the same identity under another owner, never
        // inherits the receipt.
        receipt.publish(CPU_ENTRY, 1, FOX);
        receipt.observe(OTHER_ENTRY, 1, FOX);
        assert_eq!(receipt.admit(None, OTHER_ENTRY, FOX), None);
        assert_eq!(receipt, PresentedReceipt::NONE);
        receipt.publish(CPU_ENTRY, 1, FOX);
        receipt.observe(CPU_ENTRY, 4, FOX);
        assert_eq!(receipt, PresentedReceipt::NONE);
        // Observation alone never creates a receipt.
        receipt.observe(CPU_ENTRY, 1, ROSALINA);
        assert_eq!(receipt, PresentedReceipt::NONE);
    }

    #[test]
    fn mii_class_publications_are_republished_never_admitted() {
        let mut receipt = PresentedReceipt::NONE;
        for class in 0x48..=0x4a {
            receipt.publish(CPU_ENTRY, 1, class);
            assert_eq!(receipt.admit(None, CPU_ENTRY, class), None);
            assert_eq!(
                receipt.rebuild(None, CPU_ENTRY, class),
                Rebuild::Republish((CPU_ENTRY, 1, class))
            );
            // Another class on the same entry is not this publication.
            let other = if class == 0x4a { 0x48 } else { class + 1 };
            assert_eq!(receipt.rebuild(None, CPU_ENTRY, other), Rebuild::Native);
        }
        // An exact Mii published after Fox supersedes the Fox receipt.
        receipt.publish(CPU_ENTRY, 1, FOX);
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, FOX), Rebuild::Admit((CPU_ENTRY, 1, FOX)));
        receipt.publish(CPU_ENTRY, 1, 0x4a);
        assert_eq!(receipt.admit(None, CPU_ENTRY, FOX), None);
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, FOX), Rebuild::Native);
        assert_eq!(receipt.admit(None, CPU_ENTRY, 0x4a), None);
        // A later ordinary publication supersedes the Mii one and is admitted again.
        receipt.publish(CPU_ENTRY, 1, FOX);
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, 0x4a), Rebuild::Native);
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, FOX), Rebuild::Admit((CPU_ENTRY, 1, FOX)));
    }

    #[test]
    fn css_dr_mario_to_exact_gunner_republishes_on_every_reset_until_retired() {
        const DR_MARIO: u32 = 0x12;
        const GUNNER: u32 = 0x4a;
        let mut receipt = PresentedReceipt::NONE;
        // CSS (Random) Dr. Mario: nothing published; native reset keeps its own path.
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, DR_MARIO), Rebuild::Native);
        // A replay load publishes the exact Gunner through the Mii adapter; `loaded` stays empty.
        receipt.publish(CPU_ENTRY, 1, GUNNER);
        // Every native reset of that live body republishes; the decision has no one-shot state.
        for _ in 0..3 {
            assert_eq!(
                receipt.rebuild(None, CPU_ENTRY, GUNNER),
                Rebuild::Republish((CPU_ENTRY, 1, GUNNER))
            );
        }
        // A rebuild of another live kind or another entry is not this publication.
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, DR_MARIO), Rebuild::Native);
        assert_eq!(receipt.rebuild(None, OTHER_ENTRY, GUNNER), Rebuild::Native);
        // A matching loader receipt is still the ordinary admission.
        let loaded = Some((CPU_ENTRY, 1, DR_MARIO));
        assert_eq!(receipt.rebuild(loaded, CPU_ENTRY, DR_MARIO), Rebuild::Admit((CPU_ENTRY, 1, DR_MARIO)));
        assert_eq!(receipt.rebuild(loaded, CPU_ENTRY, GUNNER), Rebuild::Republish((CPU_ENTRY, 1, GUNNER)));
        // Entry destruction, a new identity, another owner and scene exit retire it.
        receipt.retire_entry(CPU_ENTRY);
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, GUNNER), Rebuild::Native);
        receipt.publish(CPU_ENTRY, 1, GUNNER);
        receipt.observe(OTHER_ENTRY, 1, GUNNER);
        assert_eq!(receipt.rebuild(None, OTHER_ENTRY, GUNNER), Rebuild::Native);
        receipt.publish(CPU_ENTRY, 1, GUNNER);
        receipt.observe(CPU_ENTRY, 4, GUNNER);
        assert_eq!(receipt, PresentedReceipt::NONE);
        receipt.publish(CPU_ENTRY, 1, GUNNER);
        receipt.retire();
        assert_eq!(receipt.rebuild(None, CPU_ENTRY, GUNNER), Rebuild::Native);
    }

    #[test]
    fn a_matching_loader_receipt_wins_and_a_stale_one_cannot_hide_a_later_publication() {
        const MARIO: u32 = 0x00;
        let loaded_mario = Some((CPU_ENTRY, 1, MARIO));
        // Ordinary cycle only: admission is exactly the loader receipt's.
        assert_eq!(PresentedReceipt::NONE.admit(loaded_mario, CPU_ENTRY, MARIO), loaded_mario);
        assert_eq!(PresentedReceipt::NONE.admit(loaded_mario, CPU_ENTRY, FOX), None);
        // Cycle to Mario, then a replay Switch to Fox through the adapter: `loaded`
        // still names Mario and must not block Fox.
        let mut receipt = PresentedReceipt::NONE;
        receipt.publish(CPU_ENTRY, 1, FOX);
        assert_eq!(receipt.admit(loaded_mario, CPU_ENTRY, FOX), Some((CPU_ENTRY, 1, FOX)));
        assert_eq!(receipt.admit(loaded_mario, CPU_ENTRY, MARIO), loaded_mario);
        // When both name the live body, the loader's receipt is the one returned
        // (owner 4 here only tells the two receipts apart).
        let loaded_fox = Some((CPU_ENTRY, 4, FOX));
        assert_eq!(receipt.admit(loaded_fox, CPU_ENTRY, FOX), loaded_fox);
        // A later loader replacement retires the adapter receipt; only `loaded` admits.
        receipt.retire();
        assert_eq!(receipt.admit(loaded_mario, CPU_ENTRY, FOX), None);
        assert_eq!(receipt.admit(loaded_mario, CPU_ENTRY, MARIO), loaded_mario);
    }

    #[test]
    fn a_native_transformation_carries_the_adapter_receipt_to_the_live_form() {
        let mut receipt = PresentedReceipt::NONE;
        // Pokemon Trainer published through the adapter, then Squirtle -> Ivysaur.
        receipt.publish(CPU_ENTRY, 1, 0x24);
        receipt.observe(CPU_ENTRY, 1, 0x25);
        assert_eq!(receipt.admit(None, CPU_ENTRY, 0x25), Some((CPU_ENTRY, 1, 0x25)));
        assert_eq!(receipt.admit(None, CPU_ENTRY, 0x24), None);
        // Aegis: Pyra -> Mythra.
        receipt.publish(CPU_ENTRY, 1, 0x5a);
        receipt.observe(CPU_ENTRY, 1, 0x5b);
        assert_eq!(receipt.admit(None, CPU_ENTRY, 0x5b), Some((CPU_ENTRY, 1, 0x5b)));
    }
}
