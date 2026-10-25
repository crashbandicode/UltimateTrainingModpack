//! Which waiting room may open the native editor. Local Wireless is a separate
//! owner from Training and from Quickplay; it is not admitted by widening either.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Training,
    Quickplay,
    LocalWireless,
}

/// `mode` is the native mode word. Quickplay keeps the existing family mask.
/// Local Wireless waiting is exactly `0x02010000` and uses parent vtable
/// `0x50b8028`, not Training `0x50bad50` or Quickplay parent `0x50b9000`.
pub(super) fn admit(
    native_training: bool, lobby_active: bool, mode: u32,
) -> Result<Admission, &'static str> {
    if native_training {
        return Ok(Admission::Training);
    }
    if lobby_active && mode & 0x7fffff00 == 0x07010100 {
        return Ok(Admission::Quickplay);
    }
    if lobby_active && mode == 0x02010000 {
        return Ok(Admission::LocalWireless);
    }
    Err("Enter Training, Quickplay, or Local Wireless waiting practice to edit a Mii")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn training_and_quickplay_keep_their_entries() {
        assert_eq!(admit(true, false, 0x04040000), Ok(Admission::Training));
        assert_eq!(admit(true, true, 0x02010000), Ok(Admission::Training));
        assert_eq!(admit(false, true, 0x07010100), Ok(Admission::Quickplay));
        assert_eq!(admit(false, true, 0x07010102), Ok(Admission::Quickplay));
    }

    #[test]
    fn local_wireless_is_its_own_admission() {
        assert_eq!(admit(false, true, 0x02010000), Ok(Admission::LocalWireless));
        assert_ne!(admit(false, true, 0x02010000), Ok(Admission::Quickplay));
        assert_ne!(admit(false, true, 0x02010000), Ok(Admission::Training));
        assert_eq!(admit(true, true, 0x02010000), Ok(Admission::Training));
        assert_eq!(
            admit(false, false, 0x02010000),
            Err("Enter Training, Quickplay, or Local Wireless waiting practice to edit a Mii"),
        );
        assert_eq!(
            admit(false, true, 0),
            Err("Enter Training, Quickplay, or Local Wireless waiting practice to edit a Mii"),
        );
    }
}
