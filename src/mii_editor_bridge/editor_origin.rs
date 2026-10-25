//! Which waiting room may open the native editor. Local Wireless is a separate
//! owner from Training and from Quickplay; it is not admitted by widening either.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Training,
    Quickplay,
}

/// `mode` is the native mode word. Quickplay keeps the existing family mask.
/// Local Wireless waiting is exactly `0x02010000` and is refused until its own
/// scene owner is bound. Training's scene (`+0x50bad50`, state 5) and
/// Quickplay's scene (`+0x50b7998`, parent `+0x50b9000`, context state 8) are
/// not that owner.
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
        return Err("Local Wireless editor owner is not bound");
    }
    Err("Enter Training or Quickplay waiting practice to edit a Mii")
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
    fn local_wireless_does_not_reuse_either_owner() {
        assert_eq!(
            admit(false, true, 0x02010000),
            Err("Local Wireless editor owner is not bound"),
        );
        assert_eq!(
            admit(false, false, 0x02010000),
            Err("Enter Training or Quickplay waiting practice to edit a Mii"),
        );
        assert_eq!(
            admit(false, true, 0),
            Err("Enter Training or Quickplay waiting practice to edit a Mii"),
        );
    }
}
