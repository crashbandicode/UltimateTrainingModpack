//! Private test-only P1 CPU pilot. Native integration and measured ranks belong
//! to the adapter. This module makes no native reads/writes or status guesses.

pub mod config;
pub mod move_observer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Immediately,
    AfterCompletedMoves(u32),
    AfterGameFrames(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Release,
    WalkOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub level: u8,
    pub trigger: Trigger,
    pub command: Command,
    pub require_contact: bool,
    /// Release when elapsed active game frames reach this limit; zero disables
    /// control immediately. The guard takes precedence over a trigger.
    pub guard_frames: u64,
}

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=9).contains(&self.level) {
            return Err("cpu_level_out_of_range");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Completed,
    MatchEnded,
    IdentityChanged,
    GuardExpired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Piloting,
    Commanding,
    Done(End),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Pad,
    Ai,
    Command(Command),
}

/// One adapter observation. `game_frame` comes from the battle simulation,
/// not hook count or wall time. The adapter owns match/fighter/target identity.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub game_frame: u64,
    pub identity_matches: bool,
    pub match_ended: bool,
    /// False during menus/paused/inactive simulation: physical Pad, no progress.
    pub active: bool,
    /// Discrete completed attacks/specials INCLUDING whiffs observed this frame.
    /// Excludes hits, jumps, grabs, throws and other actions. Never infer missing
    /// events from a frame gap. Repeated observations of a frame are ignored.
    pub completed_moves: u32,
    pub infliction: bool,
    pub target_damage: f32,
    pub p1_lost_stock: bool,
}

#[derive(Debug)]
pub struct Pilot {
    cfg: Config,
    rank: u8,
    phase: Phase,
    frames: u64,
    moves: u64,
    hits: u64,
    damage_dealt: f32,
    last_seen_frame: Option<u64>,
    previous_active_frame: Option<u64>,
    last_output: Output,
    was_inflicting: bool,
    last_damage: Option<f32>,
    pending_hit_frame: Option<u64>,
}

impl Pilot {
    /// `ranks[level - 1]` must be measured by the native adapter; no default map.
    /// The selected-rank 0..100 bound is retained from the original draft.
    pub fn new(cfg: Config, ranks: Option<[u8; 9]>) -> Result<Self, &'static str> {
        cfg.validate()?;
        let ranks = ranks.ok_or("cpu_rank_table_unmeasured")?;
        let rank = ranks[usize::from(cfg.level - 1)];
        if rank > 100 {
            return Err("cpu_rank_out_of_range");
        }
        Ok(Self {
            cfg,
            rank,
            phase: Phase::Piloting,
            frames: 0,
            moves: 0,
            hits: 0,
            damage_dealt: 0.0,
            last_seen_frame: None,
            previous_active_frame: None,
            last_output: Output::Pad,
            was_inflicting: false,
            last_damage: None,
            pending_hit_frame: None,
        })
    }

    pub fn rank(&self) -> u8 {
        self.rank
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn elapsed_game_frames(&self) -> u64 {
        self.frames
    }
    pub fn completed_moves(&self) -> u64 {
        self.moves
    }
    pub fn damaging_hits(&self) -> u64 {
        self.hits
    }
    pub fn damage_dealt(&self) -> f32 {
        self.damage_dealt
    }

    fn trigger_met(&self) -> bool {
        match self.cfg.trigger {
            Trigger::Immediately => true,
            Trigger::AfterCompletedMoves(n) => self.moves >= u64::from(n),
            Trigger::AfterGameFrames(n) => self.frames >= n,
        }
    }

    fn gate_met(&self) -> bool {
        self.cfg.command != Command::WalkOff
            || !self.cfg.require_contact
            || (self.hits > 0 && self.damage_dealt > 0.0)
    }

    fn finish(&mut self, reason: End) -> Output {
        self.phase = Phase::Done(reason);
        self.last_output = Output::Pad;
        Output::Pad
    }

    fn clear_contact_window(&mut self) {
        self.pending_hit_frame = None;
        self.last_damage = None;
        self.was_inflicting = false;
    }

    fn observe_contact(&mut self, frame: &Frame) {
        if !frame.target_damage.is_finite() || frame.target_damage < 0.0 {
            self.clear_contact_window();
            return;
        }
        if self
            .pending_hit_frame
            .is_some_and(|edge| frame.game_frame - edge > 1)
        {
            self.pending_hit_frame = None;
        }
        if frame.infliction && !self.was_inflicting {
            self.pending_hit_frame = Some(frame.game_frame);
        }
        self.was_inflicting = frame.infliction;
        if let Some(previous) = self.last_damage {
            let delta = frame.target_damage - previous;
            if delta > 0.0 && self.pending_hit_frame.is_some() {
                self.hits = self.hits.saturating_add(1);
                self.damage_dealt += delta;
                self.pending_hit_frame = None;
            }
        }
        self.last_damage = Some(frame.target_damage);
    }

    /// Terminal identity/end signals release even on a duplicate ID. Inactive
    /// observations clear the timing baseline and return Pad. First active
    /// observation after pause is a baseline (zero time); subsequent consecutive
    /// active observations add actual ID deltas, never hook-call counts.
    pub fn step(&mut self, frame: Frame) -> Output {
        if matches!(self.phase, Phase::Done(_)) {
            return Output::Pad;
        }
        if !frame.identity_matches {
            return self.finish(End::IdentityChanged);
        }
        if frame.match_ended {
            return self.finish(End::MatchEnded);
        }
        if !frame.active {
            self.previous_active_frame = None;
            self.last_seen_frame = Some(self.last_seen_frame.unwrap_or(0).max(frame.game_frame));
            self.clear_contact_window();
            self.last_output = Output::Pad;
            return Output::Pad;
        }
        if self
            .last_seen_frame
            .is_some_and(|last| frame.game_frame <= last)
        {
            return self.last_output;
        }
        if let Some(previous) = self.previous_active_frame {
            self.frames = self.frames.saturating_add(frame.game_frame - previous);
        }
        self.previous_active_frame = Some(frame.game_frame);
        self.last_seen_frame = Some(frame.game_frame);
        self.moves = self.moves.saturating_add(u64::from(frame.completed_moves));
        self.observe_contact(&frame);

        if self.phase == Phase::Commanding && frame.p1_lost_stock {
            return self.finish(End::Completed);
        }
        if self.frames >= self.cfg.guard_frames {
            return self.finish(End::GuardExpired);
        }
        self.last_output = match self.phase {
            Phase::Piloting if self.trigger_met() && self.gate_met() => {
                if self.cfg.command == Command::Release {
                    return self.finish(End::Completed);
                }
                self.phase = Phase::Commanding;
                Output::Command(self.cfg.command)
            }
            Phase::Piloting => Output::Ai,
            Phase::Commanding => Output::Command(self.cfg.command),
            Phase::Done(_) => Output::Pad,
        };
        self.last_output
    }
}

/// Original draft encoder retained, unverified against native input at runtime.
pub fn encode(buttons: u32, stick_x: f32, stick_y: f32) -> u64 {
    let axis = |value: f32| -> u8 {
        let units = if value.is_finite() {
            (value * 120.0).round().clamp(-120.0, 120.0)
        } else {
            0.0
        };
        (units as i8) as u8
    };
    u64::from(buttons) | (u64::from(axis(stick_x)) << 32) | (u64::from(axis(stick_y)) << 40)
}

/// Existing walk-off proposal: nearer edge, then down/out in air. Native
/// ledge/drop-through behavior remains root-owned and runtime-unverified.
pub fn walk_off(p1_x: f32, left_edge: f32, right_edge: f32, grounded: bool) -> u64 {
    let out = if (right_edge - p1_x) <= (p1_x - left_edge) {
        1.0
    } else {
        -1.0
    };
    encode(0, out, if grounded { 0.0 } else { -1.0 })
}

#[cfg(test)]
mod tests;
