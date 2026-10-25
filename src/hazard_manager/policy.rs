//! A temporary waiting-rule write, containing values rather than native pointers.
#[derive(Clone, Copy)]
pub struct Override { original: u8, written: u8 }

impl Override {
    pub fn record(previous: Option<Self>, observed: u8, written: u8) -> Self {
        let original = previous.filter(|old| old.written == observed)
            .map_or(observed, |old| old.original);
        Self { original, written }
    }

    pub fn restore(self, observed: u8) -> Option<u8> {
        (observed == self.written).then_some(self.original)
    }
}

#[cfg(test)]
mod tests {
    use super::Override;

    #[test]
    fn changing_waiting_setting_retains_native_value_and_latest_write() {
        let first = Override::record(None, 1, 0);
        let second = Override::record(Some(first), 0, 1);
        let third = Override::record(Some(second), 1, 0);
        assert_eq!(third.restore(0), Some(1));
        assert_eq!(third.restore(1), None);
    }

    #[test]
    fn native_rule_replacement_is_neither_clobbered_nor_masked() {
        let first = Override::record(None, 0, 1);
        assert_eq!(first.restore(2), None);
        let next = Override::record(Some(first), 2, 0);
        assert_eq!(next.restore(0), Some(2));
    }
}
