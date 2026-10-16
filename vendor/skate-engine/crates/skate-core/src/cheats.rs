//! gm_sk8 addition: single-player cheats set by the host.
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

pub static NO_BAIL: AtomicBool = AtomicBool::new(false);
pub static NO_FALL_BAIL: AtomicBool = AtomicBool::new(false);
static GRAVITY_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

///Reasons kept under NO_BAIL: the states that would otherwise never end.
pub const ESCAPE_REASONS: [usize; 4] = [17, 24, 26, 31];
///Reasons dropped under NO_FALL_BAIL: landing or falling too hard.
pub const FALL_REASONS: [usize; 6] = [5, 6, 15, 27, 33, 9];

pub fn set_gravity_scale(scale: f32) {
    let s = if scale.is_finite() { scale.clamp(0.0, 16.0) } else { 1.0 };
    GRAVITY_SCALE.store(s.to_bits(), Relaxed);
}

pub fn gravity_scale() -> f32 {
    f32::from_bits(GRAVITY_SCALE.load(Relaxed))
}

///For predictions that divide by gravity.
pub fn prediction_gravity_scale() -> f32 {
    gravity_scale().max(0.02)
}

pub fn no_bail() -> bool {
    NO_BAIL.load(Relaxed)
}

pub fn vetoes(reason: usize) -> bool {
    if NO_BAIL.load(Relaxed) && !ESCAPE_REASONS.contains(&reason) {
        return true;
    }
    NO_FALL_BAIL.load(Relaxed) && FALL_REASONS.contains(&reason)
}
