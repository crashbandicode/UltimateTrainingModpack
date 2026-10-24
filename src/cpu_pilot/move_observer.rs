//! Engine-common move instances, not per-character status guesses.
//!
//! Numeric source: skyline-smash 01bf5835cc1140f5052ddfb2b4f9e98008f22679,
//! src/lua_const.rs FIGHTER_STATUS_KIND_* (lines 18723..19168); proposal and
//! limitations reviewed in ../CPU-CLOCK-MOVES-AUDIT.md, section 3.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Jab,
    Dash,
    SideTilt,
    UpTilt,
    DownTilt,
    SideSmash,
    DownSmash,
    UpSmash,
    Aerial,
    NeutralSpecial,
    SideSpecial,
    UpSpecial,
    DownSpecial,
}

/// Engine-common constants, identical across fighter kinds. Fighter-specific
/// statuses > FINAL may continue an existing instance but cannot open one.
pub mod status {
    pub const FALL_SPECIAL: i32 = 0x10;
    pub const LANDING_ATTACK_AIR: i32 = 0x18;
    pub const LANDING_FALL_SPECIAL: i32 = 0x19;
    pub const ATTACK: i32 = 0x27;
    pub const ATTACK_100: i32 = 0x28;
    pub const ATTACK_DASH: i32 = 0x29;
    pub const ATTACK_S3: i32 = 0x2a;
    pub const ATTACK_HI3: i32 = 0x2b;
    pub const ATTACK_LW3: i32 = 0x2c;
    pub const ATTACK_S4_START: i32 = 0x2d;
    pub const ATTACK_S4: i32 = 0x2f;
    pub const ATTACK_LW4_START: i32 = 0x30;
    pub const ATTACK_LW4: i32 = 0x32;
    pub const ATTACK_HI4_START: i32 = 0x33;
    pub const ATTACK_HI4: i32 = 0x35;
    pub const ATTACK_AIR: i32 = 0x36;
    pub const CAPTURE_PULLED: i32 = 0x41;
    pub const THROWN: i32 = 0x46;
    pub const DAMAGE: i32 = 0x47;
    pub const DAMAGE_FALL: i32 = 0x4f;
    pub const DEAD: i32 = 0xb5;
    pub const REBIRTH: i32 = 0xb6;
    pub const STANDBY: i32 = 0x1d6;
    pub const SPECIAL_N: i32 = 0x1dc;
    pub const SPECIAL_S: i32 = 0x1dd;
    pub const SPECIAL_HI: i32 = 0x1de;
    pub const SPECIAL_LW: i32 = 0x1df;
    pub const FINAL: i32 = 0x1e0;
}

pub fn family(status: i32) -> Option<Family> {
    use self::status::*;
    Some(match status {
        ATTACK | ATTACK_100 => Family::Jab,
        ATTACK_DASH => Family::Dash,
        ATTACK_S3 => Family::SideTilt,
        ATTACK_HI3 => Family::UpTilt,
        ATTACK_LW3 => Family::DownTilt,
        ATTACK_S4_START..=ATTACK_S4 => Family::SideSmash,
        ATTACK_LW4_START..=ATTACK_LW4 => Family::DownSmash,
        ATTACK_HI4_START..=ATTACK_HI4 => Family::UpSmash,
        ATTACK_AIR => Family::Aerial,
        SPECIAL_N => Family::NeutralSpecial,
        SPECIAL_S => Family::SideSpecial,
        SPECIAL_HI => Family::UpSpecial,
        SPECIAL_LW => Family::DownSpecial,
        _ => return None,
    })
}

impl Family {
    fn is_special(self) -> bool {
        matches!(
            self,
            Self::NeutralSpecial | Self::SideSpecial | Self::UpSpecial | Self::DownSpecial
        )
    }

    fn continues(self, status: i32) -> bool {
        status > status::FINAL
            || family(status) == Some(self)
            || (self == Self::Aerial && status == status::LANDING_ATTACK_AIR)
            || (self.is_special()
                && matches!(status, status::FALL_SPECIAL | status::LANDING_FALL_SPECIAL))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptionReason {
    NativeSignal,
    CommonStatus,
    /// Conservative fallback, not proof that damage always interrupts (armor).
    DamageRiseUnconfirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptedMove {
    pub family: Family,
    pub reason: InterruptionReason,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub completed: Option<Family>,
    pub interrupted: Option<InterruptedMove>,
    pub active_family: Option<Family>,
}

#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub game_frame: u64,
    pub status: i32,
    pub prev0: i32,
    pub p1_damage: f32,
    /// None: conservative damage-rise fallback. Some(true): force interruption.
    /// Some(false): verified continuation through damage, NOT a default false.
    /// Known common interrupt statuses always win over Some(false).
    pub native_interrupted: Option<bool>,
}

#[derive(Debug, Default)]
pub struct MoveObserver {
    active: Option<Family>,
    last_frame: Option<u64>,
    last_status: Option<i32>,
    last_damage: Option<f32>,
}

impl MoveObserver {
    pub fn active_family(&self) -> Option<Family> {
        self.active
    }

    /// Reset on a new bound match/fighter identity; does not invent a completion.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Once per finalized active P1 simulation observation. Duplicate/stale
    /// frames emit no events. Unobserved same-status reentry is not inferred.
    pub fn step(&mut self, observation: Observation) -> Events {
        let mut events = Events {
            active_family: self.active,
            ..Events::default()
        };
        if self
            .last_frame
            .is_some_and(|last| observation.game_frame <= last)
        {
            return events;
        }
        let changed = self.last_status != Some(observation.status);
        let damage = observation.p1_damage;
        let valid_damage = damage.is_finite() && damage >= 0.0;
        let damage_rose =
            valid_damage && self.last_damage.is_some_and(|previous| damage > previous);
        self.last_damage = if valid_damage { Some(damage) } else { None };
        self.last_frame = Some(observation.game_frame);
        self.last_status = Some(observation.status);

        let common_interrupt = matches!(observation.status,
            status::CAPTURE_PULLED..=status::THROWN | status::DAMAGE..=status::DAMAGE_FALL
            | status::DEAD | status::REBIRTH | status::STANDBY);
        let reason = if common_interrupt {
            Some(InterruptionReason::CommonStatus)
        } else if observation.native_interrupted == Some(true) {
            Some(InterruptionReason::NativeSignal)
        } else if observation.native_interrupted.is_none() && damage_rose {
            Some(InterruptionReason::DamageRiseUnconfirmed)
        } else {
            None
        };
        if let Some(reason) = reason {
            if let Some(family) = self.active.take() {
                events.interrupted = Some(InterruptedMove { family, reason });
            }
            // Never reopen a stale common/extended phase on the interrupt tick.
        } else {
            if let Some(open) = self.active {
                if changed && !open.continues(observation.status) {
                    events.completed = self.active.take();
                }
            }
            if self.active.is_none() && changed {
                self.active = family(observation.status).or_else(|| {
                    if observation.status > status::FINAL {
                        family(observation.prev0)
                    } else {
                        None
                    }
                });
            }
        }
        events.active_family = self.active;
        events
    }
}

#[cfg(test)]
#[path = "move_observer_tests.rs"]
mod tests;
