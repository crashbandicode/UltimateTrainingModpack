//! A temporary waiting-rule write, containing values rather than native pointers.
//! The receipt carries the scene tuple the native stage load ran in, so a later
//! stage load can retire exactly this receipt without touching other practice state.

/// Scene identity of a waiting-rule write: the mode tuple of the native stage load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene { pub melee: u8, pub mode: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Override { scene: Scene, original: u8, written: u8 }

impl Override {
    /// Record a write for `scene`. Repeated loads of the same scene retain the native
    /// original and update the last value written; a receipt from another scene never
    /// lends its original to this one.
    pub fn record(previous: Option<Self>, scene: Scene, observed: u8, written: u8) -> Self {
        let original = previous
            .filter(|old| old.scene == scene && old.written == observed)
            .map_or(observed, |old| old.original);
        Self { scene, original, written }
    }

    /// The native original, only while the byte still holds this receipt's last write.
    pub fn restore(self, observed: u8) -> Option<u8> {
        (observed == self.written).then_some(self.original)
    }

    /// A stage load in any other scene (a match, Training, no scene) departs this receipt.
    /// The same waiting scene reloading its stage keeps it and re-records.
    pub fn departs(self, current: Option<Scene>) -> bool {
        current != Some(self.scene)
    }
}

#[cfg(test)]
mod tests {
    use super::{Override, Scene};

    const LDN: Scene = Scene { melee: 0x0d, mode: 0x02010000 };
    const QUICKPLAY: Scene = Scene { melee: 0x0d, mode: 0x07010101 };

    #[test]
    fn changing_waiting_setting_retains_native_value_and_latest_write() {
        let first = Override::record(None, LDN, 1, 0);
        let second = Override::record(Some(first), LDN, 0, 1);
        let third = Override::record(Some(second), LDN, 1, 0);
        assert_eq!(third.restore(0), Some(1));
        assert_eq!(third.restore(1), None);
    }

    #[test]
    fn native_rule_replacement_is_neither_clobbered_nor_masked() {
        let first = Override::record(None, LDN, 0, 1);
        assert_eq!(first.restore(2), None);
        let next = Override::record(Some(first), LDN, 2, 0);
        assert_eq!(next.restore(0), Some(2));
    }

    #[test]
    fn stage_load_of_another_scene_departs_the_receipt_and_same_scene_keeps_it() {
        let receipt = Override::record(None, QUICKPLAY, 1, 0);
        // The Quickplay match keeps the mode word and changes the melee byte.
        assert!(receipt.departs(Some(Scene { melee: 0x0e, mode: 0x07010101 })));
        assert!(receipt.departs(Some(Scene { melee: 0x01, mode: 0x02010000 })));
        assert!(receipt.departs(None));
        // Repeated loads of the same waiting scene (stage switch, identical-tuple room
        // re-creation) keep the receipt; the record step then carries the original.
        assert!(!receipt.departs(Some(QUICKPLAY)));
        let again = Override::record(Some(receipt), QUICKPLAY, 0, 1);
        assert_eq!(again.restore(1), Some(1));
        // A receipt that was never departed does not lend its original to another scene.
        let other = Override::record(Some(receipt), LDN, 0, 1);
        assert_eq!(other.restore(1), Some(0));
    }
}
