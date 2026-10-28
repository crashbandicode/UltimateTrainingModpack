//! Move-aligned input crops from existing captures. This is not a pose restore:
//! the selected move and its short input setup restart from neutral Training.
//! Engine status IDs/families are supplied by the adapter, never guessed here.
use crate::{ReplayTrack, StartState};

#[derive(Clone, Debug)]
pub struct MoveFamily {
    pub fighter: Option<i32>,
    pub statuses: Vec<i32>,
    /// Required observed attack/special/grab input. Zero disables this check for
    /// adapters that cannot classify mapped controls, never for the game adapter.
    pub buttons: u32,
}

#[derive(Clone, Debug)]
pub struct MoveStartRules {
    pub families: Vec<MoveFamily>,
    /// Locomotion, jump and shield setup whose inputs may precede the attack.
    /// Damage, grabs by the opponent, respawn and ledge states are not setup.
    pub setup: Vec<i32>,
    pub ground: i32,
    pub air: i32,
    pub wait: i32,
    pub fall: i32,
    pub trigger_buttons: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveStart {
    /// First input, including setup. This becomes the visible In handle.
    pub frame: u32,
    /// First observed frame of the selected move, not an animation seek target.
    pub action_frame: u32,
    pub action_status: i32,
    /// Explicit neutral restart context, persisted by the ordinary macro format.
    pub context: StartState,
}

/// Product bound on included preparation, not an engine input-buffer constant.
/// Never turn a small crop into minutes of unrelated match playback.
pub const MAX_MOVE_SETUP: u32 = 120;

impl MoveStartRules {
    fn family(&self, fighter: i32, status: i32) -> Option<usize> {
        self.families.iter().position(|family|
            family.fighter.is_none_or(|kind| kind == fighter) && family.statuses.contains(&status))
    }

    /// Pick the move containing In, the move immediately after its setup, or a
    /// nearby earlier move. Never hunt indefinitely backward through the match.
    /// Smash start/charge/release and multi-phase specials share a family. Walk
    /// back through only the immediately adjacent setup, not through earlier
    /// attacks or damage to a distant WAIT. One preceding input is retained:
    /// state was sampled before the fighter callback, controls after it.
    pub fn snap(&self, track: &ReplayTrack, frame: u32) -> Result<MoveStart, String> {
        let at = usize::try_from(frame).ok().filter(|&i| i < track.frames.len())
            .ok_or("No captured frame at In")?;
        let fighter = track.frames[at].start.fighter_kind;
        let mut action = at;
        if self.setup.contains(&track.frames[at].start.status) {
            while action < track.frames.len()
                && track.frames[action].start.fighter_kind == fighter
                && self.setup.contains(&track.frames[action].start.status)
                && action - at < MAX_MOVE_SETUP as usize { action += 1; }
            if action >= track.frames.len() || action - at >= MAX_MOVE_SETUP as usize
                || track.frames[action].start.fighter_kind != fighter
                || self.family(fighter, track.frames[action].start.status).is_none() {
                return Err("No move immediately after this setup; move In nearer an attack".into());
            }
        }
        let family = loop {
            let sample = &track.frames[action];
            if sample.start.fighter_kind != fighter {
                return Err("No move before In in this fighter form".into());
            }
            if let Some(family) = self.family(fighter, sample.start.status) { break family; }
            if action == 0 || at.saturating_sub(action) >= MAX_MOVE_SETUP as usize {
                return Err("No recorded move near In; move In nearer an attack".into());
            }
            action -= 1;
        };
        while action > 0 {
            let before = &track.frames[action - 1];
            if before.start.fighter_kind != fighter
                || self.family(fighter, before.start.status) != Some(family) { break; }
            // A new starter after the preceding move's recovery is a new move,
            // even when both share the same multi-phase family (e.g. specials).
            if track.frames[action].start.status == self.families[family].statuses[0]
                && before.start.status != track.frames[action].start.status { break; }
            action -= 1;
        }
        if action == 0 {
            return Err("Capture begins inside this move; choose a later move".into());
        }
        let mut setup = action;
        while setup > 0 {
            let before = &track.frames[setup - 1];
            if before.start.fighter_kind != fighter || !self.setup.contains(&before.start.status) { break; }
            setup -= 1;
            if action - setup >= MAX_MOVE_SETUP as usize {
                return Err("Move setup exceeds two seconds; choose a later move start".into());
            }
        }
        let mut input = setup.saturating_sub(1);
        let required = self.families[family].buttons;
        if required != 0 && !track.frames[input..=action].iter().any(|f| f.input.buttons & required != 0) {
            let prior_status = track.frames[input].start.status;
            loop {
                if input == 0 || action - (input - 1) > MAX_MOVE_SETUP as usize {
                    return Err("No triggering input near this move; choose another start".into());
                }
                let before = &track.frames[input - 1];
                if before.start.fighter_kind != fighter || before.start.status != prior_status {
                    return Err("No triggering input captured for this move; choose another start".into());
                }
                input -= 1;
                if before.input.buttons & required != 0 { break; }
            }
        }
        // Buffered direction can disappear before the observed transition.
        // Keep the initiating press within the preceding status, including its
        // earlier stick direction (e.g. down-smash buffered during a roll).
        let held = track.frames[input].input.buttons & self.trigger_buttons;
        let prior_status = track.frames[input].start.status;
        while input > 0 && held != 0 {
            let before = &track.frames[input - 1];
            if before.start.fighter_kind != fighter || before.start.status != prior_status
                || before.input.buttons & held == 0 { break; }
            if action - (input - 1) > MAX_MOVE_SETUP as usize {
                return Err("Buffered move setup exceeds two seconds; choose another start".into());
            }
            input -= 1;
        }
        let mut context = track.frames[input].start;
        if context.fighter_kind != fighter {
            return Err("Move setup crosses a fighter transformation".into());
        }
        // A preceding endlag sample is evidence of the triggering input, not a
        // claim that endlag/work variables are restored. Start from neutral and
        // replay the short setup through normal native simulation instead.
        if context.situation == self.ground {
            context.status = self.wait;
            context.speed_x = 0.0;
            context.speed_y = 0.0;
        } else if context.situation == self.air {
            context.status = self.fall;
        } else {
            return Err("Move starts on a ledge or outside normal play; choose another start".into());
        }
        Ok(MoveStart { frame: input as u32, action_frame: action as u32,
            action_status: track.frames[action].start.status, context })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputFrame, ReplayFrame};

    fn rules() -> MoveStartRules {
        MoveStartRules { families: vec![
            MoveFamily { fighter: None, statuses: vec![45, 46, 47], buttons: 0 },
            MoveFamily { fighter: None, statuses: vec![54], buttons: 0 },
            MoveFamily { fighter: Some(74), statuses: vec![480, 486], buttons: 0 },
        ], setup: vec![6, 7, 10, 11, 12], ground: 0, air: 2, wait: 0, fall: 14,
            trigger_buttons: 0 }
    }
    fn track(statuses: &[i32]) -> ReplayTrack {
        ReplayTrack { entry: 1, quantized_analog: false, frames: statuses.iter().enumerate()
            .map(|(i, &status)| ReplayFrame { input: InputFrame { buttons: i as u32, ..Default::default() },
                start: StartState { status, ..StartState::neutral(74, 0, 1.0) }, x: 0.0, y: 0.0 }).collect() }
    }

    #[test]
    fn charged_smash_keeps_local_turn_setup_instead_of_distant_idle() {
        // Original regression: old WAIT/FALL search skips thousands of active
        // frames. A previous special ends, then turn/dash and smash charge/fire.
        let mut statuses = vec![486; 4400]; statuses[0] = 0;
        statuses.extend([6, 6, 7, 7, 7, 45, 45, 46, 47, 47]);
        let t = track(&statuses);
        assert_eq!(t.previous_clean_start(4409, &[0, 14]), Some(0));
        let start = rules().snap(&t, 4409).unwrap();
        assert_eq!((start.frame, start.action_frame, start.action_status), (4399, 4405, 45));
        assert_eq!(start.context.status, 0);
        assert_eq!(t.frames[4399].start.status, 486); // Source remains immutable.
    }

    #[test]
    fn aerial_includes_ground_jump_and_double_jump_inputs() {
        let mut t = track(&[486, 10, 10, 11, 12, 54, 54]);
        for f in &mut t.frames[3..] { f.start.situation = 2; f.start.speed_y = 3.5; }
        let start = rules().snap(&t, 6).unwrap();
        assert_eq!((start.frame, start.action_frame, start.context.situation), (0, 5, 0));
        assert_eq!(start.context.status, 0);
    }

    #[test]
    fn airborne_start_keeps_momentum_but_does_not_claim_animation_restore() {
        let mut t = track(&[14, 54, 54]);
        t.frames[0].start.situation = 2; t.frames[0].start.speed_y = 2.5;
        let start = rules().snap(&t, 2).unwrap();
        assert_eq!((start.frame, start.context.status, start.context.speed_y), (0, 14, 2.5));
    }

    #[test]
    fn keeps_buffered_direction_before_the_immediate_pre_move_frame() {
        let mut rules = rules(); rules.trigger_buttons = 1;
        let mut t = track(&[32, 32, 32, 45, 46, 47]);
        t.frames[0].input = InputFrame::default();
        t.frames[1].input = InputFrame { buttons: 1, lstick_y: -127, ..Default::default() };
        t.frames[2].input = InputFrame { buttons: 1, ..Default::default() };
        assert_eq!(rules.snap(&t, 5).unwrap().frame, 1);
    }

    #[test]
    fn consecutive_same_family_moves_snap_to_the_latest_starter() {
        let t = track(&[0, 45, 46, 47, 45, 46, 47]);
        let s = rules().snap(&t, 6).unwrap();
        assert_eq!((s.frame, s.action_frame), (3, 4));
    }

    #[test]
    fn setup_points_forward_and_long_idle_never_jumps_to_replay_beginning() {
        let t = track(&[0, 45, 47, 486, 6, 7, 45, 46, 47]);
        let s = rules().snap(&t, 5).unwrap();
        assert_eq!((s.frame, s.action_frame), (3, 6));
        let mut t = track(&vec![0; 250]); t.frames[1].start.status = 45;
        assert!(rules().snap(&t, 249).is_err());
    }

    #[test]
    fn released_buffer_and_jump_bit_are_preserved() {
        let mut rules = rules(); rules.trigger_buttons = 1 | (1 << 2);
        rules.families[0].buttons = 1;
        let mut t = track(&[32, 32, 32, 32, 45, 47]);
        for f in &mut t.frames { f.input = InputFrame::default(); }
        t.frames[1].input = InputFrame { buttons: 1, lstick_y: -127, ..Default::default() };
        assert_eq!(rules.snap(&t, 5).unwrap().frame, 1);
        t.frames[1].input.buttons = 0;
        assert!(rules.snap(&t, 5).is_err());
        let mut t = track(&[486, 486, 486, 10, 11, 54]);
        for f in &mut t.frames { f.input.buttons = 0; }
        t.frames[1].input.buttons = 1 << 2; t.frames[2].input.buttons = 1 << 2;
        assert_eq!(rules.snap(&t, 5).unwrap().frame, 1);
    }

    #[test]
    fn refuses_missing_trigger_identity_change_and_unbounded_setup() {
        assert!(rules().snap(&track(&[45, 46, 47]), 2).is_err());
        assert!(rules().snap(&track(&[0, 0, 0]), 2).is_err());
        let mut t = track(&[0, 7, 45]); t.frames[0].start.fighter_kind = 8;
        assert!(rules().snap(&t, 2).is_err());
        let mut statuses = vec![7; 122]; statuses[0] = 0; statuses.push(45);
        assert!(rules().snap(&track(&statuses), 122).is_err());
    }
}
