//! `match-data` reads a capture directory and writes Training, playback or AI data.
//!
//! Saved-replay import is `import-replay` and calls the replay adapter. It needs
//! the `replay` feature (on by default). The command does not launch a game.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use smash_match_data::adapters::{
    ai_dataset, ai_recording, playback_clip, training_macro, ButtonMap, ClipFile, ClipPoint,
    ClipSpec, LinearScale, RecordingQuery, ScaleRounding, TrainingQuantizeOptions,
};
use smash_match_data::validate::{competitive_conformance, has_errors};
use smash_match_data::{
    validate_capture, ActorId, Capture, Completeness, EventBody, SelectAt, SupplyLevel,
};
#[cfg(feature = "replay")]
use smash_match_data::{RulesetTarget, WriteOptions};

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("error: {message}");
            eprintln!();
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
        Err(CliError::Fail(message)) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

enum CliError {
    Usage(String),
    Fail(String),
}

fn usage() -> &'static str {
    "match-data — shared match recordings for Training, playback export and AI

validate <capture-dir>
inspect <capture-dir>
import-replay --version <ver> --fram <payload> --capture-id <id> --out <dir>
              (--replay-sha <64-hex> | --replay-file <path>) [--setup <header>]
              [--rules-target competitive|casual|unspecified] [--created-utc <rfc3339>]
              [--axis-order <left_x,left_y,right_x,right_y>] [--overwrite]
training|playback --capture <dir> --out <file> --actor <id>
              (--at-seq N --until-seq N | --at-tick N --until-tick N |
               --at-record N --until-record N)
              [--allow-truncated] [--clip <clip.json>]
ai --capture <dir> --out <file> [--at-seq N --until-seq N | --at-tick | --at-record]
              [--actor <id>] [--allow-truncated] [--clip <clip.json>]
training also accepts --quantize, which then requires:
              --f32-scale <spec>|none --i8-scale <spec>|none --buttons <map>
scale spec: origin,span,max_abs,half-away|toward-zero,saturate|fail
button map: identity:<1-32>  or  bits:<src-out>,<src-out>

A tick or record selection on training, playback and actor-selected ai is cut
by the selected actor's own input. The clip starts at that actor's first input
of the start tick or record (that actor's before-state stays in the context)
and stops before that actor's first input of the end tick or record. Another
fighter's earlier input on the same clock does not move the cut. An input with
no actor id does not define it, and controller numbers are not mapped to actors.
ai without --actor keeps the unscoped cut: the first input from any fighter.
Seq selection includes events with seq <= end and excludes events with seq <=
start. A record ordinal is not a gameplay tick.

import-replay is compiled only with the replay feature. Saved-file decoding
supplies record clocks and present-packet inputs; fighter state, animation and
actor identity stay unsupported. Semantic sticks stay unsupported unless
--axis-order names a permutation of stored axes 0,1,2,3. ai without --actor
exports the recording, including inputs with no actor and raw source bytes.
This tool does not capture or render a match."
}

struct Args {
    positionals: Vec<String>,
    flags: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Args {
    fn parse(raw: impl Iterator<Item = String>) -> Result<Self, CliError> {
        let mut positionals = Vec::new();
        let mut flags = Vec::new();
        let mut switches = Vec::new();
        let mut items: Vec<String> = raw.collect();
        if items.iter().any(|item| item == "--help" || item == "-h") {
            return Err(CliError::Usage("help".into()));
        }
        let known_switch = ["quantize", "allow-truncated", "overwrite"];
        let known_flag = [
            "version",
            "fram",
            "capture-id",
            "out",
            "replay-sha",
            "replay-file",
            "setup",
            "rules-target",
            "created-utc",
            "replay-size",
            "replay-path",
            "axis-order",
            "capture",
            "actor",
            "at-seq",
            "at-tick",
            "at-record",
            "until-seq",
            "until-tick",
            "until-record",
            "clip",
            "f32-scale",
            "i8-scale",
            "buttons",
        ];
        while !items.is_empty() {
            let item = items.remove(0);
            if let Some(name) = item.strip_prefix("--") {
                if let Some((key, value)) = name.split_once('=') {
                    push_flag(&mut flags, key, value, &known_flag)?;
                } else if known_switch.contains(&name) {
                    if switches.iter().any(|switch| switch == name) {
                        return Err(CliError::Usage(format!("duplicate --{name}")));
                    }
                    switches.push(name.to_string());
                } else if known_flag.contains(&name) {
                    if items.is_empty() || items[0].starts_with("--") {
                        return Err(CliError::Usage(format!("missing value for --{name}")));
                    }
                    let value = items.remove(0);
                    push_flag(&mut flags, name, &value, &known_flag)?;
                } else {
                    return Err(CliError::Usage(format!("unknown option --{name}")));
                }
            } else {
                positionals.push(item);
            }
        }
        Ok(Args {
            positionals,
            flags,
            switches,
        })
    }

    fn flag(&self, name: &str) -> Result<&str, CliError> {
        self.flags
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .ok_or_else(|| CliError::Usage(format!("missing --{name}")))
    }

    fn optional(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn switched(&self, name: &str) -> bool {
        self.switches.iter().any(|switch| switch == name)
    }

    fn allow(&self, flags: &[&str], switches: &[&str]) -> Result<(), CliError> {
        for (key, _) in &self.flags {
            if !flags.contains(&key.as_str()) {
                return Err(CliError::Usage(format!(
                    "--{key} is not valid for this command"
                )));
            }
        }
        for switch in &self.switches {
            if !switches.contains(&switch.as_str()) {
                return Err(CliError::Usage(format!(
                    "--{switch} is not valid for this command"
                )));
            }
        }
        Ok(())
    }
}

fn push_flag(
    flags: &mut Vec<(String, String)>,
    key: &str,
    value: &str,
    known: &[&str],
) -> Result<(), CliError> {
    if !known.contains(&key) {
        return Err(CliError::Usage(format!("unknown option --{key}")));
    }
    if flags.iter().any(|(existing, _)| existing == key) {
        return Err(CliError::Usage(format!("duplicate --{key}")));
    }
    flags.push((key.to_string(), value.to_string()));
    Ok(())
}

fn run(mut raw: impl Iterator<Item = String>) -> Result<(), CliError> {
    let command = raw
        .next()
        .ok_or_else(|| CliError::Usage("missing command".into()))?;
    if command == "help" || command == "--help" || command == "-h" {
        println!("{}", usage());
        return Ok(());
    }
    let args = Args::parse(raw)?;
    match command.as_str() {
        "validate" => cmd_validate(&args),
        "inspect" => cmd_inspect(&args),
        "import-replay" => cmd_import(&args),
        "training" => cmd_consumer(&args, Consumer::Training),
        "playback" => cmd_consumer(&args, Consumer::Playback),
        "ai" => cmd_ai(&args),
        other => Err(CliError::Usage(format!("unknown command {other}"))),
    }
}

fn positional_dir(args: &Args) -> Result<PathBuf, CliError> {
    match args.positionals.as_slice() {
        [dir] => Ok(PathBuf::from(dir)),
        _ => Err(CliError::Usage("expected one capture directory".into())),
    }
}

fn load_dir(dir: &Path) -> Result<Capture, CliError> {
    Capture::load(dir).map_err(|err| CliError::Fail(err.to_string()))
}

fn cmd_validate(args: &Args) -> Result<(), CliError> {
    args.allow(&[], &[])?;
    let dir = positional_dir(args)?;
    let capture = load_dir(&dir)?;
    let issues = validate_capture(&capture);
    print_summary(&capture);
    println!(
        "conformance: {}",
        conformance_line(&capture.header().rules.actual)
    );
    for issue in &issues {
        println!("{issue}");
    }
    if matches!(capture.completeness, Completeness::Incomplete { .. }) || has_errors(&issues) {
        return Err(CliError::Fail("validation failed".into()));
    }
    Ok(())
}

fn cmd_inspect(args: &Args) -> Result<(), CliError> {
    args.allow(&[], &[])?;
    let dir = positional_dir(args)?;
    let capture = load_dir(&dir)?;
    print_summary(&capture);
    println!(
        "conformance: {}",
        conformance_line(&capture.header().rules.actual)
    );
    print_inputs(&capture);
    Ok(())
}

fn print_inputs(capture: &Capture) {
    let mut inputs = 0u64;
    let mut with_actor = 0u64;
    let mut with_tick = 0u64;
    let mut with_record = 0u64;
    let mut missing_left = 0u64;
    for event in &capture.events {
        if let EventBody::Input(input) = &event.body {
            inputs += 1;
            if input.actor.is_some() {
                with_actor += 1;
            }
            if event.clocks.tick.is_some() {
                with_tick += 1;
            }
            if event.clocks.record.is_some() {
                with_record += 1;
            }
            if matches!(input.left_stick, smash_match_data::Channel::Missing) {
                missing_left += 1;
            }
        }
    }
    println!("inputs: {inputs}");
    println!("inputs_with_actor: {with_actor}");
    println!("inputs_with_tick: {with_tick}");
    println!("inputs_with_record: {with_record}");
    println!("missing_left_sticks: {missing_left}");
    if capture.header().source.kind == smash_match_data::SourceKind::FileDecode {
        println!(
            "file_decode: saved payload supplies fewer channels than a live producer; animation, fighter state and actor identity are not decoded"
        );
    }
}

fn print_summary(capture: &Capture) {
    let header = capture.header();
    println!("capture_id: {}", header.capture_id);
    println!(
        "format: {} {}",
        capture.manifest.format, capture.manifest.format_version
    );
    println!("source_kind: {}", source_name(header.source.kind));
    println!("completeness: {}", completeness_name(&capture.completeness));
    println!("events: {}", capture.events.len());
    println!("actors: {}", capture.manifest.actors.len());
    for (name, supply) in [
        ("kinematics", &header.channels.kinematics),
        ("situation", &header.channels.situation),
        ("animation", &header.channels.animation),
        ("vitals", &header.channels.vitals),
        ("inputs", &header.channels.inputs),
        ("lifecycle", &header.channels.lifecycle),
        ("objects", &header.channels.objects),
        ("random", &header.channels.random),
    ] {
        println!("channel.{name}: {}", supply_name(supply.level));
    }
    let clocks: Vec<String> = header
        .clocks
        .supplied
        .iter()
        .map(|clock| clock.to_string())
        .collect();
    println!("clocks_supplied: {}", clocks.join(","));
    for note in header.notes.iter().take(8) {
        println!("note: {note}");
    }
}

fn source_name(kind: smash_match_data::SourceKind) -> &'static str {
    match kind {
        smash_match_data::SourceKind::LiveMatch => "live_match",
        smash_match_data::SourceKind::ReplayRegeneration => "replay_regeneration",
        smash_match_data::SourceKind::FileDecode => "file_decode",
    }
}

fn completeness_name(completeness: &Completeness) -> String {
    match completeness {
        Completeness::Complete => "complete".to_string(),
        Completeness::DeclaredTruncated { reason } => match reason {
            Some(reason) => format!("declared_truncated: {reason}"),
            None => "declared_truncated".to_string(),
        },
        Completeness::Incomplete { .. } => "incomplete".to_string(),
    }
}

fn supply_name(level: SupplyLevel) -> &'static str {
    match level {
        SupplyLevel::Full => "full",
        SupplyLevel::Partial => "partial",
        SupplyLevel::Unsupported => "unsupported",
    }
}

fn conformance_line(actual: &smash_match_data::ActualRules) -> String {
    match competitive_conformance(actual) {
        smash_match_data::Conformance::Conforms => "conforms".to_string(),
        smash_match_data::Conformance::Deviates { reasons } => {
            format!("deviates: {}", reasons.join("; "))
        }
        smash_match_data::Conformance::Undetermined { unknown } => {
            format!("undetermined: {}", unknown.join(","))
        }
    }
}

fn cmd_import(args: &Args) -> Result<(), CliError> {
    import_replay_cmd(args)
}

#[cfg(feature = "replay")]
fn import_replay_cmd(args: &Args) -> Result<(), CliError> {
    use smash_match_data::replay::{import_replay_to_dir, ReplayImportRequest};
    use smash_match_data::RecorderOptions;

    args.allow(
        &[
            "version",
            "fram",
            "capture-id",
            "out",
            "replay-sha",
            "replay-file",
            "setup",
            "rules-target",
            "created-utc",
            "replay-size",
            "replay-path",
            "axis-order",
        ],
        &["overwrite"],
    )?;
    if !args.positionals.is_empty() {
        return Err(CliError::Usage(
            "import-replay takes flags, not a positional path".into(),
        ));
    }
    let version = args.flag("version")?;
    let fram_path = args.flag("fram")?;
    let capture_id = args.flag("capture-id")?;
    let out = PathBuf::from(args.flag("out")?);
    let fram = fs::read(fram_path).map_err(|err| CliError::Fail(format!("{fram_path}: {err}")))?;
    let setup = match args.optional("setup") {
        Some(path) => Some(fs::read(path).map_err(|err| CliError::Fail(format!("{path}: {err}")))?),
        None => None,
    };
    let replay = replay_association(args)?;
    let mut request = ReplayImportRequest::new(capture_id, version, &fram).with_replay(replay);
    if let Some(header) = setup.as_deref() {
        request = request.with_setup_header(header);
    }
    if let Some(created) = args.optional("created-utc") {
        request = request.with_created_utc(created);
    }
    if let Some(axes) = args.optional("axis-order") {
        let assignment = axes
            .parse::<smash_match_data::replay::AxisAssignment>()
            .map_err(|err| CliError::Usage(err.to_string()))?;
        request = request.with_axis_assignment(assignment);
    }
    request = request.with_rules_target(rules_target(args.optional("rules-target"))?);
    let options = RecorderOptions {
        write: WriteOptions {
            flush_each_event: true,
            overwrite: args.switched("overwrite"),
        },
        keep_events: false,
    };
    import_replay_to_dir(&request, &out, options).map_err(|err| CliError::Fail(err.to_string()))?;
    let loaded = load_dir(&out)?;
    println!("imported: {}", out.display());
    print_summary(&loaded);
    print_inputs(&loaded);
    Ok(())
}

#[cfg(not(feature = "replay"))]
fn import_replay_cmd(_args: &Args) -> Result<(), CliError> {
    Err(CliError::Usage(
        "import-replay requires the crate feature `replay`".into(),
    ))
}

#[cfg(feature = "replay")]
fn replay_association(args: &Args) -> Result<smash_match_data::ReplayAssociation, CliError> {
    use smash_match_data::ReplayAssociation;

    let from_file = match args.optional("replay-file") {
        Some(path) => {
            let bytes = fs::read(path).map_err(|err| CliError::Fail(format!("{path}: {err}")))?;
            Some(ReplayAssociation::from_bytes(
                &bytes,
                Some(path.to_string()),
            ))
        }
        None => None,
    };
    let from_sha = args.optional("replay-sha").map(|sha| ReplayAssociation {
        sha256: sha.to_string(),
        size_bytes: args
            .optional("replay-size")
            .and_then(|text| text.parse().ok()),
        path_hint: args.optional("replay-path").map(str::to_string),
        header_version_word: None,
        stored_seed_word: None,
    });
    match (from_file, from_sha) {
        (Some(file), Some(sha)) => {
            if file.sha256 != sha.sha256.to_ascii_lowercase() {
                return Err(CliError::Fail(
                    "replay-sha does not match replay-file".into(),
                ));
            }
            Ok(file)
        }
        (Some(file), None) => Ok(file),
        (None, Some(sha)) => Ok(sha),
        (None, None) => Err(CliError::Usage(
            "pass --replay-sha or --replay-file for the whole saved replay; the payload digest is not that hash"
                .into(),
        )),
    }
}

#[cfg(feature = "replay")]
fn rules_target(value: Option<&str>) -> Result<RulesetTarget, CliError> {
    match value.unwrap_or("unspecified") {
        "competitive" => Ok(RulesetTarget::Competitive),
        "casual" => Ok(RulesetTarget::Casual),
        "unspecified" => Ok(RulesetTarget::Unspecified),
        other => Err(CliError::Usage(format!("unknown rules target {other}"))),
    }
}

enum Consumer {
    Training,
    Playback,
}

fn cmd_consumer(args: &Args, consumer: Consumer) -> Result<(), CliError> {
    let switches: &[&str] = match consumer {
        Consumer::Training => &["quantize", "allow-truncated"],
        Consumer::Playback => &["allow-truncated"],
    };
    let mut flags = vec![
        "capture",
        "out",
        "actor",
        "at-seq",
        "at-tick",
        "at-record",
        "until-seq",
        "until-tick",
        "until-record",
        "clip",
    ];
    if matches!(consumer, Consumer::Training) {
        flags.extend(["f32-scale", "i8-scale", "buttons"]);
    }
    args.allow(&flags, switches)?;
    if !args.positionals.is_empty() {
        return Err(CliError::Usage("unexpected positional argument".into()));
    }
    let capture_dir = PathBuf::from(args.flag("capture")?);
    let out = PathBuf::from(args.flag("out")?);
    let capture = load_dir(&capture_dir)?;
    let spec = clip_spec(args)?;
    let json = match consumer {
        Consumer::Training => {
            let quantize = quantize_options(args)?;
            let macro_ = training_macro(&capture, &spec, quantize.as_ref())
                .map_err(|err| CliError::Fail(err.to_string()))?;
            serde_json::to_string_pretty(&macro_).map_err(|err| CliError::Fail(err.to_string()))?
        }
        Consumer::Playback => {
            let clip =
                playback_clip(&capture, &spec).map_err(|err| CliError::Fail(err.to_string()))?;
            serde_json::to_string_pretty(&clip).map_err(|err| CliError::Fail(err.to_string()))?
        }
    };
    write_text(&out, &json)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn cmd_ai(args: &Args) -> Result<(), CliError> {
    args.allow(
        &[
            "capture",
            "out",
            "actor",
            "at-seq",
            "at-tick",
            "at-record",
            "until-seq",
            "until-tick",
            "until-record",
            "clip",
        ],
        &["allow-truncated"],
    )?;
    if !args.positionals.is_empty() {
        return Err(CliError::Usage("unexpected positional argument".into()));
    }
    let capture_dir = PathBuf::from(args.flag("capture")?);
    let out = PathBuf::from(args.flag("out")?);
    let capture = load_dir(&capture_dir)?;
    let json = if args.optional("actor").is_some() || args.optional("clip").is_some() {
        let spec = clip_spec(args)?;
        let rows = ai_dataset(&capture, &spec).map_err(|err| CliError::Fail(err.to_string()))?;
        serde_json::to_string_pretty(&rows).map_err(|err| CliError::Fail(err.to_string()))?
    } else {
        let query = recording_query(args)?;
        let rows = ai_recording(&capture, &query).map_err(|err| CliError::Fail(err.to_string()))?;
        serde_json::to_string_pretty(&rows).map_err(|err| CliError::Fail(err.to_string()))?
    };
    write_text(&out, &json)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn recording_query(args: &Args) -> Result<RecordingQuery, CliError> {
    let ranged = [
        "at-seq",
        "at-tick",
        "at-record",
        "until-seq",
        "until-tick",
        "until-record",
    ]
    .iter()
    .any(|name| args.optional(name).is_some());
    if !ranged {
        return Ok(RecordingQuery::whole());
    }
    let start = one_point(args, "at-seq", "at-tick", "at-record")?;
    let end = one_point(args, "until-seq", "until-tick", "until-record")?;
    if std::mem::discriminant(&start) != std::mem::discriminant(&end) {
        return Err(CliError::Usage(
            "start and end must use the same clock (seq, tick or record)".into(),
        ));
    }
    Ok(RecordingQuery {
        actor: None,
        start: Some(point_at(start)),
        end: Some(point_at(end)),
    })
}

fn clip_spec(args: &Args) -> Result<ClipSpec, CliError> {
    let selection = [
        "actor",
        "at-seq",
        "at-tick",
        "at-record",
        "until-seq",
        "until-tick",
        "until-record",
    ]
    .iter()
    .any(|name| args.optional(name).is_some());
    if args.optional("clip").is_some() && selection {
        return Err(CliError::Usage(
            "--clip replaces --actor and the range flags".into(),
        ));
    }
    if let Some(path) = args.optional("clip") {
        let text =
            fs::read_to_string(path).map_err(|err| CliError::Fail(format!("{path}: {err}")))?;
        let file: ClipFile =
            serde_json::from_str(&text).map_err(|err| CliError::Fail(format!("{path}: {err}")))?;
        return Ok(ClipSpec::from(&file));
    }
    let actor = args
        .flag("actor")?
        .parse::<u32>()
        .map_err(|_| CliError::Usage("actor must be an integer id".into()))?;
    let start = one_point(args, "at-seq", "at-tick", "at-record")?;
    let end = one_point(args, "until-seq", "until-tick", "until-record")?;
    if std::mem::discriminant(&start) != std::mem::discriminant(&end) {
        return Err(CliError::Usage(
            "start and end must use the same clock (seq, tick or record)".into(),
        ));
    }
    let mut spec = ClipSpec::new(ActorId(actor), point_at(start), point_at(end));
    if args.switched("allow-truncated") {
        spec = spec.allow_declared_truncation();
    }
    Ok(spec)
}

fn one_point(args: &Args, seq: &str, tick: &str, record: &str) -> Result<ClipPoint, CliError> {
    let chosen = [seq, tick, record]
        .into_iter()
        .filter(|name| args.optional(name).is_some())
        .collect::<Vec<_>>();
    if chosen.len() != 1 {
        return Err(CliError::Usage(format!(
            "pass exactly one of --{seq}, --{tick}, --{record}"
        )));
    }
    let name = chosen[0];
    let value: u64 = args
        .flag(name)?
        .parse()
        .map_err(|_| CliError::Usage(format!("--{name} must be an integer")))?;
    Ok(match name {
        n if n == seq => ClipPoint::Seq(value),
        n if n == tick => ClipPoint::Tick(value),
        _ => ClipPoint::Record(value),
    })
}

fn point_at(point: ClipPoint) -> SelectAt {
    point.into()
}

fn quantize_options(args: &Args) -> Result<Option<TrainingQuantizeOptions>, CliError> {
    if !args.switched("quantize") {
        if args.optional("f32-scale").is_some()
            || args.optional("i8-scale").is_some()
            || args.optional("buttons").is_some()
        {
            return Err(CliError::Usage(
                "scale and button flags require --quantize".into(),
            ));
        }
        return Ok(None);
    }
    let options = TrainingQuantizeOptions {
        f32_scale: parse_scale(args.flag("f32-scale")?, "f32-scale")?,
        i8_scale: parse_scale(args.flag("i8-scale")?, "i8-scale")?,
        buttons: parse_buttons(args.flag("buttons")?)?,
    };
    options
        .checked()
        .map_err(|err| CliError::Fail(err.to_string()))?;
    Ok(Some(options))
}

fn parse_scale(text: &str, name: &str) -> Result<Option<LinearScale>, CliError> {
    if text == "none" {
        return Ok(None);
    }
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 5 {
        return Err(CliError::Usage(format!(
            "--{name} must be none or origin,span,max_abs,rounding,saturate|fail"
        )));
    }
    let origin = parse_f32(parts[0], name)?;
    let span = parse_f32(parts[1], name)?;
    let max_abs: u8 = parts[2]
        .parse()
        .map_err(|_| CliError::Usage(format!("--{name} max_abs must be an integer")))?;
    let rounding = match parts[3] {
        "half-away" | "half_away" => ScaleRounding::HalfAwayFromZero,
        "toward-zero" | "toward_zero" => ScaleRounding::TowardZero,
        other => {
            return Err(CliError::Usage(format!(
                "--{name} unknown rounding {other}"
            )))
        }
    };
    let saturate = match parts[4] {
        "saturate" => true,
        "fail" => false,
        other => return Err(CliError::Usage(format!("--{name} unknown mode {other}"))),
    };
    Ok(Some(LinearScale {
        origin,
        span,
        max_abs,
        rounding,
        saturate,
    }))
}

fn parse_f32(text: &str, name: &str) -> Result<f32, CliError> {
    let value: f32 = text
        .parse()
        .map_err(|_| CliError::Usage(format!("--{name} has a non-numeric component {text}")))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(CliError::Usage(format!("--{name} must be finite")))
    }
}

fn parse_buttons(text: &str) -> Result<ButtonMap, CliError> {
    if let Some(width) = text.strip_prefix("identity:") {
        let width_bits: u8 = width
            .parse()
            .map_err(|_| CliError::Usage("identity width must be an integer".into()))?;
        return Ok(ButtonMap::Identity { width_bits });
    }
    let Some(list) = text.strip_prefix("bits:") else {
        return Err(CliError::Usage(
            "buttons must be identity:<width> or bits:<src-out>,...".into(),
        ));
    };
    let mut routes = Vec::new();
    if !list.is_empty() {
        for pair in list.split(',') {
            let Some((src, dst)) = pair.split_once('-') else {
                return Err(CliError::Usage(format!("bad button route {pair}")));
            };
            let source = src
                .parse()
                .map_err(|_| CliError::Usage(format!("bad button route {pair}")))?;
            let output = dst
                .parse()
                .map_err(|_| CliError::Usage(format!("bad button route {pair}")))?;
            routes.push(smash_match_data::adapters::BitRoute { source, output });
        }
    }
    Ok(ButtonMap::Bits { routes })
}

fn write_text(path: &Path, json: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| CliError::Fail(format!("{}: {err}", parent.display())))?;
        }
    }
    let mut file = fs::File::create(path)
        .map_err(|err| CliError::Fail(format!("{}: {err}", path.display())))?;
    file.write_all(json.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|err| CliError::Fail(format!("{}: {err}", path.display())))?;
    Ok(())
}
