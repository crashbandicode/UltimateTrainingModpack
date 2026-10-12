//! Native observations retained before the Training controller projection.

use crate::{InputFrame, StartState};

/// ControlModule readings, before rounding or clamping to Training's i8 format.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveInput {
    pub buttons: u32,
    /// Left x/y, then right x/y, in the original native f32 representation.
    pub axes: [f32; 4],
}

impl LiveInput {
    pub fn is_finite(self) -> bool { self.axes.iter().all(|value| value.is_finite()) }

    /// Conversion belongs only at the Training consumer boundary. It must not
    /// replace the original native observations in a shared recording.
    pub fn project(self) -> (InputFrame, bool) {
        let axes = self.axes.map(|axis| {
            let value = axis * 120.0;
            let rounded = value.round().clamp(i8::MIN as f32, i8::MAX as f32);
            (rounded as i8, (value - rounded).abs() > 0.001)
        });
        let [lx, ly, rx, ry] = axes;
        (InputFrame { buttons: self.buttons, lstick_x: lx.0, lstick_y: ly.0,
            rstick_x: rx.0, rstick_y: ry.0 }, axes.iter().any(|axis| axis.1))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LiveFrame {
    pub input: LiveInput,
    pub start: StartState,
    pub x: f32,
    pub y: f32,
    pub position_z: f32,
    pub speed_z: f32,
    pub body: u32,
}

impl LiveFrame {
    pub fn is_valid(self) -> bool {
        self.start.fighter_kind >= 0 && self.input.is_finite()
            && [self.x, self.y, self.position_z, self.speed_z, self.start.facing,
                self.start.speed_x, self.start.speed_y].iter().all(|value| value.is_finite())
    }
}

#[derive(Clone, Debug)]
pub struct LiveTrack {
    pub entry: i32,
    pub frames: Vec<LiveFrame>,
}

#[derive(Clone, Debug, Default)]
pub struct LiveSession {
    pub tracks: Vec<LiveTrack>,
    pub truncated: bool,
    /// Opaque native serializer counter, not a proven gameplay/input/replay clock.
    /// Admitted tracks start together and advance contiguously without wrap.
    pub first_counter: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_retains_the_training_rounding_and_asymmetric_clamp() {
        let source = LiveInput { buttons: 513, axes: [-2.0, 2.0, -0.5 / 120.0, 0.49999997 / 120.0] };
        let before = source.axes.map(f32::to_bits);
        let (projected, quantized) = source.project();
        assert_eq!((projected.lstick_x, projected.lstick_y), (-128, 127));
        assert_eq!((projected.rstick_x, projected.rstick_y), (-1, 0));
        assert_eq!(projected.buttons, 513);
        assert!(quantized);
        assert_eq!(source.axes.map(f32::to_bits), before);
    }
}
