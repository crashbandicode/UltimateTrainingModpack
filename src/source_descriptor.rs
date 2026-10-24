//! Pure validation of a managed descriptor against current native evidence.
//! The caller authorizes reset references only for an owned Training replacement.
//! This module never reads or writes native memory.
pub(super) fn matches(
    current: [u32; 28],
    mut expected: [u32; 28],
    originals: Option<[[u64; 2]; 4]>,
    selection_base: u64,
) -> bool {
    if current == expected {
        return true;
    }
    let Some(originals) = originals else {
        return false;
    };
    let [rules, selections] = originals[0];
    if rules == 0
        || rules & 7 != 0
        || selection_base == 0
        || selections != selection_base
        || originals.iter().any(|pair| *pair != [rules, selections])
    {
        return false;
    }
    // main+1510114/+1510134 write exactly these two borrowed references.
    // Preserve exact comparison of the role/configuration fields and +68.
    expected[22] = rules as u32;
    expected[23] = (rules >> 32) as u32;
    expected[24] = selections as u32;
    expected[25] = (selections >> 32) as u32;
    current == expected
}

#[cfg(test)]
mod tests {
    use super::matches;

    // Managed02 #353, Mythra entry7 generation3 after one native LRA reset.
    const RESET: [u32; 28] = [
        0,
        0,
        0,
        0,
        0xffff_ffff,
        2,
        0x5b,
        0x5a,
        0xffff_ffff,
        0xffff_ff00,
        1,
        1,
        0,
        0,
        0x7f90_00cf,
        4,
        0xff,
        0,
        0,
        0,
        0x0100_0001,
        0,
        0x030f_0060,
        0x12,
        0x855a_1730,
        0,
        0,
        0,
    ];
    const PAIR: [u64; 2] = [0x12_030f_0060, 0x855a_1730];

    fn original() -> [u32; 28] {
        let mut value = RESET;
        value[22..26].fill(0);
        value
    }

    #[test]
    fn native_reset_receipt_requires_authorized_current_originals() {
        let before = original();
        assert!(matches(before, before, None, PAIR[1]));
        // The captured failure is accepted only after all four current originals
        // independently attest to the native-written pair.
        assert!(!matches(RESET, before, None, PAIR[1]));
        assert!(matches(RESET, before, Some([PAIR; 4]), PAIR[1]));
        for index in 0..4 {
            let mut originals = [PAIR; 4];
            originals[index][0] ^= 0x1000;
            assert!(!matches(RESET, before, Some(originals), PAIR[1]));
        }
        assert!(!matches(RESET, before, Some([PAIR; 4]), PAIR[1] + 0x260));
        assert!(!matches(RESET, before, Some([[0, PAIR[1]]; 4]), PAIR[1]));
        assert!(!matches(
            RESET,
            before,
            Some([[PAIR[0] + 1, PAIR[1]]; 4]),
            PAIR[1]
        ));
    }

    #[test]
    fn reset_does_not_authorize_other_descriptor_changes() {
        let before = original();
        for word in 0..28 {
            let mut changed = RESET;
            changed[word] ^= 1;
            assert!(
                !matches(changed, before, Some([PAIR; 4]), PAIR[1]),
                "unexpected descriptor word {} was accepted", word
            );
        }
        // An independently recreated scene can carry a new native rules pointer;
        // the previous receipt is never authority for that new current pair.
        let next = [PAIR[0] + 0x1000, PAIR[1]];
        assert!(!matches(RESET, before, Some([next; 4]), PAIR[1]));
        let mut next_receipt = RESET;
        next_receipt[22] = next[0] as u32;
        next_receipt[23] = (next[0] >> 32) as u32;
        assert!(matches(next_receipt, before, Some([next; 4]), PAIR[1]));
    }
}
