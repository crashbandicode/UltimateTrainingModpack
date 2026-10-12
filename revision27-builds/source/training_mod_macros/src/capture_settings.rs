//! INI setting that gates live-match capture independently of archive browsing
//! and native Vault fallback.

/// Example/default capture configuration. An absent setting also defaults off.
pub const DEFAULT_CAPTURE_INI: &str = "[capture]\nenabled=off\n";

/// Whether the runtime should capture a live match for later archiving.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureSettings {
    pub enabled: bool,
}

impl CaptureSettings {
    /// Parses the capture flag from an INI document without performing I/O.
    /// Unknown sections and keys are ignored; duplicate `[capture] enabled`
    /// keys and values outside the supported on/off forms are errors.
    pub fn parse(contents: &str) -> Result<Self, String> {
        let mut in_capture_section = false;
        let mut enabled_value = None;

        for (line_index, raw_line) in contents.lines().enumerate() {
            let line_number = line_index + 1;
            let line = raw_line
                .split([';', '#'])
                .next()
                .unwrap_or("")
                .trim();
            if line.is_empty() {
                continue;
            }

            if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                in_capture_section = section.trim().eq_ignore_ascii_case("capture");
                continue;
            }

            if in_capture_section && line.eq_ignore_ascii_case("enabled") {
                return Err(format!(
                    "invalid [capture] enabled value on line {line_number}; expected on/off, true/false, or 1/0"
                ));
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if !in_capture_section || !key.trim().eq_ignore_ascii_case("enabled") {
                continue;
            }
            if enabled_value.is_some() {
                return Err(format!(
                    "duplicate [capture] enabled value on line {line_number}"
                ));
            }

            let value = value.trim();
            let enabled = if value.eq_ignore_ascii_case("on")
                || value.eq_ignore_ascii_case("true")
                || value == "1"
            {
                true
            } else if value.eq_ignore_ascii_case("off")
                || value.eq_ignore_ascii_case("false")
                || value == "0"
            {
                false
            } else {
                return Err(format!(
                    "invalid [capture] enabled value {value:?} on line {line_number}; expected on/off, true/false, or 1/0"
                ));
            };
            enabled_value = Some(enabled);
        }

        Ok(Self {
            enabled: enabled_value.unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_disabled_and_documents_the_ini_form() {
        assert_eq!(CaptureSettings::default(), CaptureSettings { enabled: false });
        assert_eq!(
            DEFAULT_CAPTURE_INI,
            "[capture]\nenabled=off\n"
        );
        assert_eq!(CaptureSettings::parse("# no setting\n").unwrap(), CaptureSettings::default());
    }

    #[test]
    fn parses_supported_values_case_insensitively_with_comments() {
        for value in ["on", "ON", "true", "TrUe", "1"] {
            let settings = CaptureSettings::parse(&format!(
                "[capture]\nenabled = {value} ; live match only\n"
            ))
            .unwrap();
            assert!(settings.enabled, "value {value:?}");
        }
        for value in ["off", "OFF", "false", "FaLsE", "0"] {
            let settings = CaptureSettings::parse(&format!(
                "[capture]\nenabled={value} # capture disabled\n"
            ))
            .unwrap();
            assert!(!settings.enabled, "value {value:?}");
        }
    }

    #[test]
    fn ignores_unrelated_sections_keys_and_global_values() {
        let settings = CaptureSettings::parse(
            "enabled=on\n[other]\nenabled=on\n[capture]\nunknown=value\n[other-again]\nenabled=off\n",
        )
        .unwrap();
        assert_eq!(settings, CaptureSettings::default());
    }

    #[test]
    fn rejects_invalid_and_duplicate_capture_values_clearly() {
        let invalid = CaptureSettings::parse("[capture]\nenabled=maybe\n").unwrap_err();
        assert!(invalid.contains("invalid [capture] enabled value"));
        assert!(invalid.contains("on/off, true/false, or 1/0"));
        assert!(CaptureSettings::parse("[capture]\nenabled\n")
            .unwrap_err()
            .contains("invalid [capture] enabled value"));

        let duplicate = CaptureSettings::parse(
            "[capture]\nenabled=on\n[other]\nenabled=off\n[capture]\nenabled=off\n",
        )
        .unwrap_err();
        assert!(duplicate.contains("duplicate [capture] enabled value"));
    }
}
