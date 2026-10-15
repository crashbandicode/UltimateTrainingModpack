//! Native entry descriptors and form indices. Values only; callers own entry lifetime.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forms {
    pub count: u32,
    pub kinds: [u32; 3],
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
            _ => (1, [kind, u32::MAX, u32::MAX], 0),
        };
        Self {
            count,
            kinds,
            flags,
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
        let forms = Forms::starting(self.kinds[0]);
        (self.count == forms.count
            && self.kinds == forms.kinds
            && self.actor_count == 1
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
