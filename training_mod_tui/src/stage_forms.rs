//! Value-only practice stage choices. Native mapping is owned by the consts catalog.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Requested form choices. Final Destination form is an alias for Omega.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Form {
    #[default]
    Normal = 0,
    #[serde(alias = "final_destination")]
    Omega = 1,
    Battlefield = 2,
}

impl Form {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Omega, Self::Battlefield];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Omega => "Omega / Final Destination",
            Self::Battlefield => "Battlefield",
        }
    }

    /// Only the catalog's supported forms participate; a special stage stays Normal.
    pub fn next(self, native_forms: [Option<u32>; 3]) -> Self {
        (1..=Self::ALL.len()).map(|step| Self::ALL[(self as usize + step) % Self::ALL.len()])
            .find(|form| native_forms[*form as usize].is_some()).unwrap_or(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Base catalog identity, not a resolved native alternate-form ID.
    pub stage: u32,
    #[serde(default)]
    pub form: Form,
}

impl Selection {
    pub const fn normal(stage: u32) -> Self { Self { stage, form: Form::Normal } }
}

impl From<u32> for Selection {
    fn from(stage: u32) -> Self { Self::normal(stage) }
}

/// One form per existing ranked stage. Ranks and catalog indices never change.
/// The catalog owner verifies this length and every stage/form pair at ingress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forms([Form; 117]);

impl Forms {
    pub const NORMAL: Self = Self([Form::Normal; 117]);
    pub fn get(&self, index: usize) -> Option<Form> { self.0.get(index).copied() }
    pub fn set(&mut self, index: usize, form: Form) -> Result<(), &'static str> {
        *self.0.get_mut(index).ok_or("stage form index outside catalog")? = form;
        Ok(())
    }
    pub fn as_slice(&self) -> &[Form] { &self.0 }
}

impl Default for Forms {
    fn default() -> Self { Self::NORMAL }
}

impl Serialize for Forms {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_slice().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Forms {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let forms = Vec::<Form>::deserialize(deserializer)?;
        let mut result = Self::NORMAL;
        if forms.len() != result.0.len() {
            return Err(serde::de::Error::custom("invalid stage form catalog length"));
        }
        result.0.copy_from_slice(&forms);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_destination_alias_has_one_canonical_serialization() {
        let omega: Selection = serde_json::from_str(r#"{"stage":95,"form":"omega"}"#).unwrap();
        let fd: Selection = serde_json::from_str(r#"{"stage":95,"form":"final_destination"}"#).unwrap();
        assert_eq!(fd, omega);
        assert_eq!(serde_json::to_string(&fd).unwrap(), r#"{"stage":95,"form":"omega"}"#);
        assert_eq!(serde_json::from_str::<Selection>(r#"{"stage":95}"#).unwrap(), Selection::normal(95));
        for bad in [r#"{"stage":95,"form":"fd_unknown"}"#, r#"{"stage":95,"form":3}"#,
            r#"{"stage":95,"form":null}"#, r#"{"stage":95,"forms":"omega"}"#] {
            assert!(serde_json::from_str::<Selection>(bad).is_err());
        }
        assert_eq!(serde_json::from_str::<Selection>(r#"{"stage":95,"form":"battlefield"}"#).unwrap(),
            Selection { stage: 95, form: Form::Battlefield });
    }

    #[test]
    fn form_edit_roundtrip_preserves_other_cells_and_rejects_bad_restore() {
        let mut forms = Forms::default();
        forms.set(1, Form::Omega).unwrap();
        forms.set(5, Form::Battlefield).unwrap();
        let restored: Forms = serde_json::from_str(&serde_json::to_string(&forms).unwrap()).unwrap();
        assert_eq!(restored, forms);
        assert_eq!(restored.get(0), Some(Form::Normal));
        assert_eq!(restored.get(116), Some(Form::Normal));
        assert!(forms.set(117, Form::Omega).is_err());
        assert_eq!(restored.get(117), None);
        assert!(serde_json::from_str::<Forms>("[]").is_err());
        assert_eq!(Form::Normal.next([Some(308), None, None]), Form::Normal);
        assert_eq!(Form::Normal.next([Some(0), Some(1), None]), Form::Omega);
        assert_eq!(Form::Omega.next([Some(0), Some(1), None]), Form::Normal);
        assert_eq!(Form::Normal.next([Some(3), None, Some(4)]), Form::Battlefield);
        assert_eq!(Form::Omega.next([Some(95), Some(96), Some(97)]), Form::Battlefield);
        assert_eq!(Form::Battlefield.next([Some(95), Some(96), Some(97)]), Form::Normal);
    }
}
