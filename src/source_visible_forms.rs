//! Value-only qualification of one visible native alternate-form input route.
//! The native caller reacquires every member; these addresses are equality
//! evidence and are never dereferenced here. Pending-load epoch ownership stays
//! with the existing SourceToken/Preflight/INSTALL contract.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Member {
    pub(super) actor: usize,
    pub(super) control: usize,
    pub(super) kind: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bank {
    pub(super) entry: usize,
    pub(super) owner: u32,
    pub(super) resource: (usize, u32, u32),
    pub(super) kinds: [u32; 3],
    pub(super) bodies: [(usize, u32); 3],
    pub(super) members: [Option<Member>; 3],
    pub(super) active: usize,
    pub(super) record: usize,
    // Actual active control's input_index, binding, alias_index.
    pub(super) route: (i32, usize, i32),
}

pub(super) type Slot = (usize, u8, i32, i32, i32);

fn binding(bank: Bank) -> Option<(usize, i32)> {
    // Only Aegis and Trainer are alternate forms. IC/SoloPopo and ordinary
    // copy/character changes cannot obtain permission through this predicate.
    let family: &[u32] = match bank.kinds[0] {
        0x5a | 0x5b => &[0x5a, 0x5b],
        0x24..=0x26 => &[0x24, 0x25, 0x26],
        _ => return None,
    };
    let count = family.len();
    if !matches!(bank.owner, 0 | 1 | 4)
        || bank.entry == 0
        || bank.resource.0 == 0
        || bank.resource.1 != bank.kinds[0]
        || bank.record == 0
        || bank.route != (bank.owner as i32, bank.record, bank.owner as i32)
        || bank.active >= count
        || bank.kinds[..count].iter().any(|kind| !family.contains(kind))
        || bank.kinds[count..].iter().any(|kind| *kind != u32::MAX)
    {
        return None;
    }
    for index in 0..3 {
        if index >= count {
            if bank.members[index].is_some() || bank.bodies[index] != (0, 0) {
                return None;
            }
            continue;
        }
        let member = bank.members[index]?;
        if member.actor == 0 || member.control == 0
            || bank.bodies[index].0 == 0 || !family.contains(&member.kind)
            || bank.kinds[..index].contains(&bank.kinds[index])
            || bank.bodies[..index].iter().any(|body| body.0 == bank.bodies[index].0)
            || bank.members[..index].iter().flatten().any(|previous| {
                previous.actor == member.actor || previous.control == member.control
                    || previous.kind == member.kind
            })
        {
            return None;
        }
    }
    let active = bank.members[bank.active]?;
    Some((active.control, active.kind as i32))
}

/// Derive an expected row from independent native member authority. Preserve
/// occupancy and owner/alias fields exactly; never copy current input as truth.
pub(super) fn expected_slot(previous: Slot, before: Bank, current: Bank) -> Option<Slot> {
    let old_binding = binding(before)?;
    let new_binding = binding(current)?;
    let mut stable = current;
    stable.active = before.active;
    if stable != before || (previous.0, previous.2) != old_binding {
        return None;
    }
    Some((new_binding.0, previous.1, new_binding.1, previous.3, previous.4))
}

/// Exact merge used by the native caller. Optional authority never changes the
/// unchanged strict path. Changed rows need same-owner native bank evidence;
/// state/owner/alias and all other rows remain the strict expected values.
pub(super) fn merge_slots(
    mut expected: [Slot; 16], observed: [Slot; 16], owners: [u32; 2],
    before: [Option<Bank>; 2], current: [Option<Bank>; 2],
) -> Option<[Slot; 16]> {
    if expected == observed { return Some(expected); }
    if !matches!(owners, [0, 1] | [0, 4]) { return None; }
    for (index, owner) in owners.iter().copied().enumerate() {
        let slot = owner as usize;
        if expected[slot] == observed[slot] { continue; }
        let previous = before[index]?;
        let active = current[index]?;
        if previous.owner != owner || active.owner != owner { return None; }
        expected[slot] = expected_slot(expected[slot], previous, active)?;
    }
    (expected == observed).then_some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The two route values are the exact LDN05 B #531/#556 input failure.
    // Other bank fields are explicit native-reader contract fixtures, not a
    // claim that B logged each actor/control field (it did not).
    fn bank(kinds: [u32; 3], owner: u32) -> Bank {
        let count = if kinds[2] == u32::MAX { 2 } else { 3 };
        let mut result = Bank {
            entry: 0x12_0be0_4000, owner, resource: (0x11_faf7_5100, kinds[0], 0),
            kinds, bodies: [(0, 0); 3], members: [None; 3], active: 0,
            record: 0x11_f543_f010 + owner as usize * 0x280,
            route: (0, 0, 0),
        };
        result.route = (owner as i32, result.record, owner as i32);
        for (index, kind) in kinds.iter().copied().enumerate().take(count) {
            result.bodies[index] = (0x1000 + index * 0x100, index as u32);
            result.members[index] = Some(Member {
                actor: 0x2000 + index * 0x80, control: 0x3000 + index * 0x100, kind,
            });
        }
        result
    }

    fn row(bank: Bank) -> Slot {
        let member = bank.members[bank.active].unwrap();
        (member.control, if bank.owner == 0 { 0 } else { 1 }, member.kind as i32,
            if bank.owner == 0 { -1 } else { bank.owner as i32 },
            if bank.owner == 0 { -1 } else { bank.owner as i32 })
    }

    fn rows(bank: Bank) -> [Slot; 16] {
        let mut rows = [(0, 2, -1, -1, -1); 16];
        rows[bank.owner as usize] = row(bank);
        rows
    }

    fn authority(bank: Bank) -> ([u32; 2], [Option<Bank>; 2]) {
        if bank.owner == 0 { ([0, 4], [Some(bank), None]) }
        else { ([0, bank.owner], [None, Some(bank)]) }
    }

    #[test]
    fn captured_ldn05_aegis_delta_requires_same_native_bank() {
        let mut before = bank([0x5b, 0x5a, u32::MAX], 0);
        // Canonical actor order is Pyra/Mythra even when body order starts Mythra.
        before.members.swap(0, 1);
        before.active = 1;
        before.members[0].as_mut().unwrap().control = 0x11_dba6_0c00;
        before.members[1].as_mut().unwrap().control = 0x11_80ee_2e00;
        let previous = (0x11_80ee_2e00, 0, 0x5b, -1, -1);
        let observed = (0x11_dba6_0c00, 0, 0x5a, -1, -1);
        assert_ne!(previous, observed); // The unchanged strict comparator rejects B.
        let mut after = before; after.active = 0;
        assert_eq!(expected_slot(previous, before, after), Some(observed));
        assert_eq!(merge_slots(rows(before), rows(after), [0, 4],
            [Some(before), None], [Some(after), None]), Some(rows(after)));
        let mut wrong = after; wrong.entry += 8;
        assert_eq!(expected_slot(previous, before, wrong), None);
        wrong = after; wrong.members[0].as_mut().unwrap().control += 8;
        assert_eq!(expected_slot(previous, before, wrong), None);
    }

    #[test]
    fn trainer_all_normal_switch_edges_for_visible_player_and_cpu() {
        for owner in [0, 1, 4] {
            for start in 0..3 {
                let mut before = bank([0x25, 0x26, 0x24], owner); before.active = start;
                let mut after = before; after.active = (start + 1) % 3;
                assert_eq!(expected_slot(row(before), before, after), Some(row(after)));
                let (owners, saved) = authority(before);
                assert_eq!(merge_slots(rows(before), rows(after), owners, saved, authority(after).1), Some(rows(after)));
            }
        }
    }

    #[test]
    fn stable_control_and_return_to_previous_form_are_exact() {
        for owner in [0, 1, 4] {
            let before = bank([0x5b, 0x5a, u32::MAX], owner);
            assert_eq!(expected_slot(row(before), before, before), Some(row(before)));
            let mut after = before; after.active = 1;
            assert_eq!(expected_slot(row(before), before, after), Some(row(after)));
            assert_eq!(expected_slot(row(after), after, before), Some(row(before)));
        }
    }

    #[test]
    fn changed_lifetime_group_body_or_control_authority_is_refused() {
        let before = bank([0x25, 0x26, 0x24], 4);
        let mut after = before; after.active = 1;
        let reject = |bad| assert_eq!(expected_slot(row(before), before, bad), None);
        let mut bad = after; bad.entry += 8; reject(bad);
        bad = after; bad.owner = 0; reject(bad);
        bad = after; bad.resource.0 += 8; reject(bad);
        bad = after; bad.resource.2 += 1; reject(bad);
        bad = after; bad.bodies[1].1 += 1; reject(bad);
        bad = after; bad.members[1].as_mut().unwrap().actor += 8; reject(bad);
        bad = after; bad.members[1].as_mut().unwrap().kind = 0x5a; reject(bad);
        bad = after; bad.route.0 = 7; reject(bad);
        bad = after; bad.route.1 += 0x280; reject(bad);
        bad = after; bad.route.2 = 7; reject(bad);
        bad = after; bad.active = 3; reject(bad);
        bad = after; bad.kinds.swap(0, 1); reject(bad);
    }

    #[test]
    fn ic_solopopo_kirby_and_bowser_gain_no_pointer_change_exception() {
        for kinds in [[0x4b, 0x4c, u32::MAX], [0x4b, u32::MAX, u32::MAX],
                      [6, u32::MAX, u32::MAX], [0xf, u32::MAX, u32::MAX]] {
            let before = bank(kinds, 0);
            let mut after = before; after.active = 1;
            assert_eq!(expected_slot(row(before), before, after), None);
        }
    }

    #[test]
    fn forged_old_input_is_refused() {
        let before = bank([0x5b, 0x5a, u32::MAX], 0);
        let mut after = before; after.active = 1;
        let original = row(before); let expected = expected_slot(original, before, after).unwrap();
        assert_eq!(expected, row(after));
        let mut forged = original; forged.0 += 8;
        assert_eq!(expected_slot(forged, before, after), None);
        forged = original; forged.2 = 6;
        assert_eq!(expected_slot(forged, before, after), None);
    }

    #[test]
    fn merge_refuses_state_owner_alias_and_other_row_changes() {
        for owner in [0, 1, 4] {
            let before = bank([0x5b, 0x5a, u32::MAX], owner);
            let mut after = before; after.active = 1;
            let expected = rows(before); let observed = rows(after);
            let (owners, saved) = authority(before);
            let merge = |observed| merge_slots(expected, observed, owners, saved, authority(after).1);
            let slot = owner as usize;
            assert_eq!(merge(observed), Some(observed));
            let mut bad = observed; bad[slot].1 ^= 1; assert_eq!(merge(bad), None);
            bad = observed; bad[slot].3 = 7; assert_eq!(merge(bad), None);
            bad = observed; bad[slot].4 = 7; assert_eq!(merge(bad), None);
            bad = observed; bad[slot].0 += 8; assert_eq!(merge(bad), None);
            bad = observed; bad[slot].2 = 6; assert_eq!(merge(bad), None);
            for other in [0, 1, 4, 7, 15].iter().copied().filter(|other| *other != slot) {
                bad = observed; bad[other].0 = 0x1230; assert_eq!(merge(bad), None);
            }
            bad = observed; bad[7].1 = 0; assert_eq!(merge(bad), None);
        }
    }

    #[test]
    fn missing_authority_preserves_strict_path_but_never_accepts_a_delta() {
        let before = bank([0x5b, 0x5a, u32::MAX], 0);
        let mut after = before; after.active = 1;
        assert_eq!(merge_slots(rows(before), rows(before), [0, 4], [None; 2], [None; 2]), Some(rows(before)));
        assert_eq!(merge_slots(rows(before), rows(after), [0, 4], [None; 2], [Some(after), None]), None);
        assert_eq!(merge_slots(rows(before), rows(after), [0, 4], [Some(before), None], [None; 2]), None);
    }

    #[test]
    fn invalid_owner_and_duplicate_member_controls_cannot_authorize_merge() {
        let mut before = bank([0x5b, 0x5a, u32::MAX], 7);
        let mut after = before; after.active = 1;
        assert_eq!(expected_slot(row(before), before, after), None);
        assert_eq!(merge_slots(rows(before), rows(after), [0, 7], [None, Some(before)], [None, Some(after)]), None);
        before = bank([0x5b, 0x5a, u32::MAX], 0);
        let first_control = before.members[0].unwrap().control;
        before.members[1].as_mut().unwrap().control = first_control;
        after = before; after.active = 1;
        assert_eq!(merge_slots(rows(before), rows(after), [0, 4], [Some(before), None], [Some(after), None]), None);
    }
}
