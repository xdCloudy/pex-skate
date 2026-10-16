//! gm_sk8 addition: a skater pose straight from the animation banks (a
//! clip, a posture, a gesture on top), for the add-on's settings preview.
use crate::skater_animation::AnimationSource;
use skate_core::animation::playback_tree::PoseCommand;
use std::{path::Path, sync::Arc};

fn source(root: &Path) -> Result<Arc<AnimationSource>, String> {
    AnimationSource::cached(root).ok_or_else(|| "the skater's animations aren't loaded yet".to_string())
}

pub fn clip_names(root: &Path) -> Result<Vec<String>, String> {
    let s = AnimationSource::load(root)?;
    Ok(s.evaluator().frames.clip_names().map(str::to_owned).collect())
}

pub fn bone_names(root: &Path) -> Result<Vec<String>, String> {
    Ok(source(root)?.evaluator().frames.bone_names.clone())
}

pub fn load(root: &Path) -> Result<(), String> {
    AnimationSource::load(root).map(|_| ())
}

pub fn clip_seconds(root: &Path, name: &str) -> Result<f32, String> {
    let s = source(root)?;
    let c = s.evaluator().frames.clip(name)?;
    let fps = f32::from_bits(c.fps_bits);
    Ok(if fps > 0.0 { c.frames.len().saturating_sub(1) as f32 / fps } else { 0.0 })
}

pub struct Layer<'a> {
    pub clip: &'a str,
    pub time: f32,
}

/// Model-space bone matrices (column-major), the skater's own root at the origin.
pub fn pose(root: &Path, base: Layer, posture: Option<&str>, gesture: Option<Layer>, mirror: bool) -> Result<Vec<[f32; 16]>, String> {
    let s = source(root)?;
    let evaluator = s.evaluator();
    let clip = |l: &Layer| PoseCommand::Clip { name: l.clip.into(), previous_time: l.time, time: l.time, loops: 0 };
    let mut commands = vec![clip(&base)];
    if let Some(p) = posture {
        commands.push(PoseCommand::Pose { name: p.into() });
        commands.push(PoseCommand::Add { motion_is_a: true });
    }
    commands.push(PoseCommand::Pose { name: "RIG_TPOSE".into() });
    commands.push(PoseCommand::Add { motion_is_a: true });
    if let Some(g) = gesture {
        commands.push(clip(&g));
        commands.push(PoseCommand::Pose { name: "RIG_TPOSE".into() });
        commands.push(PoseCommand::Add { motion_is_a: true });
        commands.push(PoseCommand::ChannelBlend { weight: 1.0, use_channels_from_weights: false });
    }
    if mirror {
        commands.push(PoseCommand::Mirror { trajectory_mode: 2 });
    }
    let local = evaluator.evaluate(&commands)?;
    Ok(evaluator.hierarchy(&local)?.into_iter().map(|m| crate::animation::native_matrix(m).to_cols_array()).collect())
}

pub const IDLE: &str = "NB_STAND_0_CYC";
pub const GESTURE_LOOPS: u32 = 2;
pub const GESTURE_REST: f32 = 0.8;

pub fn posture_pose(posture: u32) -> Option<&'static str> {
    match posture {
        1 => Some("POSTURE_STIFF_POSE"),
        2 => Some("POSTURE_SLOUCH_POSE"),
        3 => Some("POSTURE_BUFF_POSE"),
        _ => None,
    }
}

pub fn gesture_prefix(id: u32) -> Option<&'static str> {
    crate::graph_host::motion_character_gesture::NAMES.get(id as usize).copied()
}

fn looped(t: f32, length: f32) -> f32 {
    if length > 0.0 { t.rem_euclid(length) } else { 0.0 }
}

/// Where a looping gesture is at `t`: into, cycled a few times, out, a rest.
pub fn gesture_at(root: &Path, id: u32, t: f32) -> Result<Option<(String, f32)>, String> {
    let Some(prefix) = gesture_prefix(id) else { return Ok(None) };
    let stage = |s: &str| format!("{prefix}_BOTH_{s}");
    let (into, cyc, out) = (stage("INTO"), stage("CYC"), stage("OUT"));
    let (a, b, c) = (clip_seconds(root, &into)?, clip_seconds(root, &cyc)?, clip_seconds(root, &out)?);
    let cycles = b * GESTURE_LOOPS as f32;
    let mut t = looped(t, a + cycles + c + GESTURE_REST);
    if t < a {
        return Ok(Some((into, t)));
    }
    t -= a;
    if t < cycles {
        return Ok(Some((cyc, looped(t, b))));
    }
    t -= cycles;
    if t < c {
        return Ok(Some((out, t)));
    }
    Ok(None)
}

/// The settings preview: standing on foot, the posture, the gesture's loop.
pub fn settings_pose(root: &Path, posture: u32, gesture: Option<u32>, t: f32, mirror: bool) -> Result<Vec<[f32; 16]>, String> {
    let idle = looped(t, clip_seconds(root, IDLE)?);
    let g = match gesture {
        Some(id) => gesture_at(root, id, t)?,
        None => None,
    };
    pose(
        root,
        Layer { clip: IDLE, time: idle },
        posture_pose(posture),
        g.as_ref().map(|(name, time)| Layer { clip: name, time: *time }),
        mirror,
    )
}
