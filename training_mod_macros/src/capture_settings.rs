//! INI setting that gates live-match capture independently of archive browsing
//! and native Vault fallback.
//!
//! The file is the single startup authority: Training reads it once to seed the
//! runtime preference and never reads it while sampling. With the Deluxe plugin
//! built and a host present, the Plugins page changes the in-memory preference
//! and the host rewrites this file after each accepted edit, provided the layout
//! is [`CaptureSettings::persistence_compatible`]. Without a host, users hand-edit
//! the file as before.

use std::{fs, io, path::Path};

/// Example/default capture configuration. An absent setting also defaults off.
pub const DEFAULT_CAPTURE_INI: &str = "[capture]\nenabled=off\n";

/// INI key the Deluxe host persistence writer replaces (`OptionSpec::config_key`).
pub const CAPTURE_CONFIG_KEY: &str = "enabled";

/// Whether the runtime should capture a live match for later archiving.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureSettings {
    pub enabled: bool,
}

/// Training's reading of one document: the recognised `[capture] enabled` line
/// (0-based index) and its value.
struct Scan {
    enabled: Option<(usize, bool)>,
}

fn scan(contents: &str) -> Result<Scan, String> {
    let mut in_capture_section = false;
    let mut enabled = None;

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

        if in_capture_section && line.eq_ignore_ascii_case(CAPTURE_CONFIG_KEY) {
            return Err(format!(
                "invalid [capture] enabled value on line {line_number}; expected on/off, true/false, or 1/0"
            ));
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !in_capture_section || !key.trim().eq_ignore_ascii_case(CAPTURE_CONFIG_KEY) {
            continue;
        }
        if enabled.is_some() {
            return Err(format!(
                "duplicate [capture] enabled value on line {line_number}"
            ));
        }

        let value = value.trim();
        let on = if value.eq_ignore_ascii_case("on")
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
        enabled = Some((line_index, on));
    }

    Ok(Scan { enabled })
}

impl CaptureSettings {
    /// Parses the capture flag from an INI document without performing I/O.
    /// Unknown sections and keys are ignored; duplicate `[capture] enabled`
    /// keys and values outside the supported on/off forms are errors.
    pub fn parse(contents: &str) -> Result<Self, String> {
        let scan = scan(contents)?;
        Ok(Self {
            enabled: scan.enabled.is_some_and(|(_, on)| on),
        })
    }

    /// Whether the pinned Deluxe host may persist menu edits into this document.
    ///
    /// The host writer is section-unaware, compares the key exactly and strips
    /// only `#` comments: it replaces the first line whose key is `enabled`,
    /// otherwise it appends a line at the end of the file. Training parses
    /// case-insensitively, strips `;` and `#`, and honours `[capture]` only.
    /// Compatibility therefore requires that the document parses, that the host
    /// finds exactly one `enabled` line, and that this line is the one Training
    /// recognises as `[capture] enabled`. The host's append path is never relied
    /// on. Any other layout keeps the toggle session-only (no host persistence);
    /// the error names the reason for the startup log.
    pub fn persistence_compatible(contents: &str) -> Result<(), String> {
        let scan = scan(contents)?;
        let host: Vec<usize> = contents
            .lines()
            .enumerate()
            .filter(|(_, line)| host_persistence_key(line) == Some(CAPTURE_CONFIG_KEY))
            .map(|(index, _)| index)
            .collect();
        match (host.as_slice(), scan.enabled) {
            ([line], Some((recognised, _))) if *line == recognised => Ok(()),
            ([], _) => Err("no `enabled=` line for the host to replace; it would append outside Training's parse".into()),
            ([line], None) => Err(format!(
                "line {} is `enabled=` for the host but not [capture] enabled for Training",
                line + 1
            )),
            ([line], Some((recognised, _))) => Err(format!(
                "the host would rewrite line {} but Training reads [capture] enabled from line {}",
                line + 1,
                recognised + 1
            )),
            (lines, _) => Err(format!(
                "{} `enabled=` lines; the host would rewrite only the first (line {})",
                lines.len(),
                lines[0] + 1
            )),
        }
    }
}

/// Key the pinned Deluxe host persistence writer derives from one INI line:
/// the `#` comment is stripped, the line is split at the first `=` and the key
/// is trimmed. The host compares it exactly (case-sensitive); `;` is not a
/// comment to it.
pub fn host_persistence_key(line: &str) -> Option<&str> {
    let line = line.split('#').next().unwrap_or("");
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

/// Outcome of [`recover_previous_ini`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IniRecovery {
    /// The INI exists; nothing was touched.
    Present,
    /// Only the host's `.previous.ini` backup existed and was renamed back.
    Recovered,
    /// Neither the INI nor a usable backup exists.
    Absent,
}

/// The Deluxe host persists by writing `<stem>.tmp`, renaming the current file
/// to `<stem>.previous.ini` and renaming the temporary into place. A crash
/// between those renames leaves only the backup, so restore it before creating
/// a default file. `*.tmp` is left alone (it may be incomplete). Mirrors the
/// host's `config_store::recover`; the paths use the host's `with_extension`.
pub fn recover_previous_ini(path: &Path) -> io::Result<IniRecovery> {
    match fs::symlink_metadata(path) {
        Ok(_) => return Ok(IniRecovery::Present),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let previous = path.with_extension("previous.ini");
    match fs::symlink_metadata(&previous) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::rename(&previous, path)?;
            Ok(IniRecovery::Recovered)
        }
        Ok(_) => Ok(IniRecovery::Absent),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(IniRecovery::Absent),
        Err(error) => Err(error),
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

    /// Model of the pinned host writer (`apply_ini_assignment`): `#`-stripped,
    /// exact key, section-unaware; replaces the first matching line in full,
    /// else appends `key=value` after a terminating newline.
    fn host_rewrite(contents: &str, key: &str, value: &str) -> String {
        let assignment = format!("{key}={value}");
        let mut lines: Vec<String> = contents.lines().map(str::to_owned).collect();
        match lines.iter().position(|line| host_persistence_key(line) == Some(key)) {
            Some(index) => lines[index] = assignment,
            None => lines.push(assignment),
        }
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    const COMPATIBLE: &[&str] = &[
        DEFAULT_CAPTURE_INI,
        "[capture]\nenabled=on\n",
        "[capture]\nenabled = on ; live match only\n",
        "[capture]\nenabled=off # capture disabled\n",
        "[CAPTURE]\n enabled = On\n",
        "# header\n[capture]\n# enabled=on is written by Deluxe\nenabled=off\n[other]\nkey=1\n",
        "[capture]\nenabled=on\n[other]\nmode=b\nunrelated=enabled\n",
        "[capture]\nenabled=1\n",
    ];

    const INCOMPATIBLE: &[(&str, &str)] = &[
        ("", "no `enabled=` line"),
        ("# nothing\n[capture]\n", "no `enabled=` line"),
        ("[capture]\n;enabled=on\n", "no `enabled=` line"),
        ("[capture]\nEnabled=on\n", "no `enabled=` line"),
        ("enabled=on\n", "not [capture] enabled"),
        ("[other]\nenabled=on\n", "not [capture] enabled"),
        ("enabled=on\n[capture]\nenabled=off\n", "2 `enabled=` lines"),
        ("[other]\nenabled=on\n[capture]\nenabled=off\n", "2 `enabled=` lines"),
        ("[capture]\nenabled=off\n[other]\nenabled=on\n", "2 `enabled=` lines"),
        ("[capture]\nenabled=on\n[capture]\nenabled=off\n", "duplicate [capture] enabled"),
        ("[capture]\nenabled=maybe\n", "invalid [capture] enabled value"),
        ("[capture]\nenabled=on\n[capture]\nEnabled=off\n", "duplicate [capture] enabled"),
    ];

    #[test]
    fn host_rewrite_of_each_value_parses_back_without_duplicates() {
        for fixture in COMPATIBLE {
            CaptureSettings::persistence_compatible(fixture)
                .unwrap_or_else(|reason| panic!("{fixture:?}: {reason}"));
            let mut text = (*fixture).to_owned();
            for value in ["on", "off", "off", "on"] {
                text = host_rewrite(&text, CAPTURE_CONFIG_KEY, value);
                let parsed = CaptureSettings::parse(&text)
                    .unwrap_or_else(|error| panic!("{fixture:?} -> {text:?}: {error}"));
                assert_eq!(parsed.enabled, value == "on", "{fixture:?} -> {text:?}");
                let matches = text
                    .lines()
                    .filter(|line| host_persistence_key(line) == Some(CAPTURE_CONFIG_KEY))
                    .count();
                assert_eq!(matches, 1, "{fixture:?} -> {text:?}");
                assert_eq!(CaptureSettings::persistence_compatible(&text), Ok(()), "{text:?}");
                assert_eq!(text.lines().count(), fixture.lines().count(), "{text:?}");
            }
        }
    }

    #[test]
    fn incompatible_layouts_are_refused_with_the_reason() {
        for (fixture, reason) in INCOMPATIBLE {
            let error = CaptureSettings::persistence_compatible(fixture)
                .expect_err(&format!("{fixture:?} must be refused"));
            assert!(error.contains(reason), "{fixture:?}: {error:?} lacks {reason:?}");
        }
        // Why the refusals matter: the host rewrite would not take effect for Training.
        let mixed_case = host_rewrite("[capture]\nEnabled=on\n", CAPTURE_CONFIG_KEY, "off");
        assert!(CaptureSettings::parse(&mixed_case).unwrap_err().contains("duplicate"));
        let other_first = host_rewrite("[other]\nenabled=on\n[capture]\nenabled=on\n", CAPTURE_CONFIG_KEY, "off");
        assert!(CaptureSettings::parse(&other_first).unwrap().enabled);
        let commented = host_rewrite("[capture]\n;enabled=on\n", CAPTURE_CONFIG_KEY, "on");
        assert!(CaptureSettings::parse(&commented).unwrap().enabled);
        let appended_elsewhere = host_rewrite("[capture]\n;enabled=on\n[other]\n", CAPTURE_CONFIG_KEY, "on");
        assert!(!CaptureSettings::parse(&appended_elsewhere).unwrap().enabled);
    }

    #[test]
    fn host_key_tokenization_matches_the_pinned_writer() {
        assert_eq!(host_persistence_key("enabled=on"), Some("enabled"));
        assert_eq!(host_persistence_key("  enabled = on # note"), Some("enabled"));
        assert_eq!(host_persistence_key("enabled=on ; note"), Some("enabled"));
        assert_eq!(host_persistence_key(";enabled=on"), Some(";enabled"));
        assert_eq!(host_persistence_key("Enabled=on"), Some("Enabled"));
        assert_eq!(host_persistence_key("# enabled=on"), None);
        assert_eq!(host_persistence_key("[capture]"), None);
        assert_eq!(host_persistence_key("enabled"), None);
    }

    #[test]
    fn previous_ini_is_restored_only_when_the_ini_is_missing() {
        let temp = crate::test_util::TempDir::new();
        let ini = temp.path().join("replay_capture.ini");
        let previous = temp.path().join("replay_capture.previous.ini");
        let tmp = temp.path().join("replay_capture.tmp");
        assert_eq!(previous, ini.with_extension("previous.ini"));
        assert_eq!(tmp, ini.with_extension("tmp"));

        assert_eq!(recover_previous_ini(&ini).unwrap(), IniRecovery::Absent);
        fs::write(&tmp, "[capture]\nenabled=on\n").unwrap();
        assert_eq!(recover_previous_ini(&ini).unwrap(), IniRecovery::Absent);
        assert!(!ini.exists() && tmp.exists(), "an incomplete temporary is never adopted");

        fs::write(&previous, "[capture]\nenabled=on\n").unwrap();
        assert_eq!(recover_previous_ini(&ini).unwrap(), IniRecovery::Recovered);
        assert!(!previous.exists());
        assert_eq!(fs::read_to_string(&ini).unwrap(), "[capture]\nenabled=on\n");
        assert!(tmp.exists());

        fs::write(&previous, "[capture]\nenabled=off\n").unwrap();
        assert_eq!(recover_previous_ini(&ini).unwrap(), IniRecovery::Present);
        assert_eq!(fs::read_to_string(&ini).unwrap(), "[capture]\nenabled=on\n");
        assert!(previous.exists());

        fs::remove_file(&ini).unwrap();
        fs::remove_file(&previous).unwrap();
        fs::create_dir(&previous).unwrap();
        assert_eq!(recover_previous_ini(&ini).unwrap(), IniRecovery::Absent);
        assert!(!ini.exists());
    }
}
