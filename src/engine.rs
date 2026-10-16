//! Adapter between the PEX input protocol and SkateGM's portable host for the
//! recovered Skate 3 simulation. The host crate has no renderer; presentation
//! data crosses to Fabric as ordinary state payloads.

use crate::physics::Controls;
use skate_host::bridge::{self, Session};
use std::path::Path;

pub const SKATER_BONE_NAMES: [&str; 36] = [
    "TRAJECTORY",
    "HIPS",
    "SPINE",
    "SPINE1",
    "SPINE2",
    "SPINE3",
    "NECK",
    "NECK1",
    "HEAD",
    "RIGHTSHOULDER",
    "RIGHTARM",
    "RIGHTFOREARM",
    "RIGHTHAND",
    "LEFTSHOULDER",
    "LEFTARM",
    "LEFTFOREARM",
    "LEFTHAND",
    "RIGHTUPLEG",
    "RIGHTLEG",
    "RIGHTFOOT",
    "RIGHTTOEBASE",
    "LEFTUPLEG",
    "LEFTLEG",
    "LEFTFOOT",
    "LEFTTOEBASE",
    "SKATEBOARD_ROOT",
    "TRUCK_FRONT",
    "RIGHT_WHEELFRONT",
    "LEFT_WHEELFRONT",
    "TRUCK_BACK",
    "LEFT_WHEELBACK",
    "RIGHT_WHEELBACK",
    "RIGHTTOEBASE_REPARENTED",
    "LEFTTOEBASE_REPARENTED",
    "RIGHTHAND_REPARENTED",
    "LEFTHAND_REPARENTED",
];

pub struct SkateSession {
    session: Session,
    bone_names: Vec<String>,
    bones_root_relative: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct SkateBones {
    pub hips: [f32; 3],
    pub present: u64,
    /// Quantized x/y/z offsets from hips, in 1/4096 Minecraft-block units.
    pub offsets: [[i16; 3]; 36],
}

#[derive(Clone, Debug)]
pub struct SkateCamera {
    /// Camera origin relative to the simulation root, in Minecraft world axes.
    pub offset: [f32; 3],
    /// Skate camera basis `at` (+Z) and `up` (+Y) vectors.
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub fov: f32,
}

#[derive(Clone, Debug)]
pub struct SkateVisualPose {
    pub bones: SkateBones,
    pub camera: Option<SkateCamera>,
}

#[derive(Clone, Debug)]
pub struct SkatePose {
    pub root: [f32; 3],
    pub velocity: [f32; 3],
    pub board: [f32; 7],
    pub trick_name: String,
    pub sequence_score: f32,
    pub line_score: f32,
    pub total_score: f32,
    pub multiplier: f32,
    pub line_time: f32,
    pub clean: bool,
    pub sketchy: bool,
    pub visual: Option<SkateVisualPose>,
}

impl SkateSession {
    pub fn load(
        asset_root: &Path,
        collision_triangles: Vec<[[f32; 3]; 3]>,
        rails: Vec<Vec<[f32; 3]>>,
        spawn: [f32; 3],
        heading: f32,
    ) -> Result<Self, String> {
        let mut session = Session::new(asset_root, collision_triangles, rails, spawn, heading)?;
        let initial_pose = session.activate(spawn, heading)?;
        Ok(Self {
            session,
            bone_names: initial_pose.names,
            bones_root_relative: None,
        })
    }

    /// One deterministic 60 Hz engine step. The caller owns the server tick
    /// accumulator and must not call this directly from client packet cadence.
    pub fn tick(&mut self, controls: Controls) -> Result<SkatePose, String> {
        self.session.tick(to_engine_controls(controls))?;
        Ok(self.pose())
    }

    pub fn period(&self) -> f32 {
        self.session.period()
    }

    pub fn refresh_collision(
        &mut self,
        collision_triangles: Vec<[[f32; 3]; 3]>,
        rails: Vec<Vec<[f32; 3]>>,
    ) -> Result<(), String> {
        let prepared = self
            .session
            .collision_builder()
            .build(collision_triangles, rails)?;
        self.session.install_collision(prepared)
    }

    /// Read the full render pose at network cadence, not at physics cadence.
    /// The joint list matches SkateGM's human-rig retarget protocol.
    pub fn skeletal_pose(&mut self) -> Option<SkateVisualPose> {
        let pose = self.session.pose();
        let hips_index = self.bone_names.iter().position(|name| name == "HIPS")?;
        let hips_matrix = pose.bones.get(hips_index)?.to_cols_array();
        let root_matrix = pose.root.to_cols_array();
        let raw_hips = translation(&hips_matrix);
        let composed_hips = translation(&multiply(&root_matrix, &hips_matrix));
        let root = translation(&root_matrix);
        let reference = pose
            .camera
            .as_ref()
            .map(|(position, _, _)| position.to_array())
            .unwrap_or(root);
        let root_relative = if distance(root, [0.0; 3]) < 8.0 {
            self.bones_root_relative.unwrap_or_else(|| {
                distance(composed_hips, reference) + 0.01 < distance(raw_hips, reference)
            })
        } else {
            distance(composed_hips, reference) + 0.01 < distance(raw_hips, reference)
        };
        self.bones_root_relative = Some(root_relative);

        let hips = if root_relative {
            composed_hips
        } else {
            raw_hips
        };
        let mut present = 0_u64;
        let mut offsets = [[0_i16; 3]; 36];
        for (slot, name) in SKATER_BONE_NAMES.iter().enumerate() {
            let Some(index) = self.bone_names.iter().position(|bone| bone == name) else {
                continue;
            };
            let Some(matrix) = pose.bones.get(index) else {
                continue;
            };
            let local = matrix.to_cols_array();
            let world = if root_relative {
                multiply(&root_matrix, &local)
            } else {
                local
            };
            let point = translation(&world);
            if !point.into_iter().all(f32::is_finite) {
                continue;
            }
            present |= 1_u64 << slot;
            offsets[slot] = std::array::from_fn(|axis| {
                ((point[axis] - hips[axis]) * 4096.0)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16
            });
        }
        let camera = pose.camera.as_ref().and_then(|(position, basis, fov)| {
            let camera = SkateCamera {
                offset: [
                    position[0] - root[0],
                    position[1] - root[1],
                    position[2] - root[2],
                ],
                forward: basis.z_axis.to_array(),
                up: basis.y_axis.to_array(),
                fov: *fov,
            };
            (camera
                .offset
                .iter()
                .chain(camera.forward.iter())
                .chain(camera.up.iter())
                .all(|value| value.is_finite())
                && camera.fov.is_finite()
                && camera.fov > 0.0)
                .then_some(camera)
        });
        Some(SkateVisualPose {
            bones: SkateBones {
                hips,
                present,
                offsets,
            },
            camera,
        })
    }

    pub fn pose(&self) -> SkatePose {
        let (root, velocity, board) = self.session.kinematics();
        let score = self.session.scoring();
        SkatePose {
            root,
            velocity,
            board,
            trick_name: score.trick_name,
            sequence_score: score.sequence_score,
            line_score: score.line_score,
            total_score: score.total_score,
            multiplier: score.multiplier,
            line_time: score.line_time,
            clean: score.clean,
            sketchy: score.sketchy,
            visual: None,
        }
    }
}

fn translation(matrix: &[f32; 16]) -> [f32; 3] {
    [matrix[12], matrix[13], matrix[14]]
}

fn multiply(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| {
        let column = index / 4;
        let row = index % 4;
        (0..4)
            .map(|inner| a[inner * 4 + row] * b[column * 4 + inner])
            .sum()
    })
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn to_engine_controls(controls: Controls) -> bridge::Controls {
    const A: u16 = 1 << 12;
    const B: u16 = 1 << 13;
    const X: u16 = 1 << 14;
    let axis = |value: f32| (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
    bridge::Controls {
        buttons: (if controls.push { A } else { 0 })
            | (if controls.brake { B } else { 0 })
            | (if controls.push_left { X } else { 0 }),
        triggers: controls.triggers,
        left: [axis(controls.steer), axis(controls.forward)],
        right: [axis(controls.right_x), axis(controls.right_y)],
    }
}

#[cfg(test)]
mod tests {
    use super::to_engine_controls;
    use crate::physics::Controls;

    #[test]
    fn input_axes_and_trick_actions_map_to_the_engine_pad_packet() {
        let input = to_engine_controls(Controls {
            forward: 1.0,
            steer: -0.5,
            right_x: -1.0,
            right_y: 1.0,
            triggers: [96, 255],
            push: true,
            push_left: true,
            brake: true,
        });
        assert_eq!(input.buttons, (1 << 12) | (1 << 13) | (1 << 14));
        assert_eq!(input.triggers, [96, 255]);
        assert_eq!(input.left[0], -16_384);
        assert_eq!(input.left[1], i16::MAX);
        assert_eq!(input.right, [-i16::MAX, i16::MAX]);
    }
}
