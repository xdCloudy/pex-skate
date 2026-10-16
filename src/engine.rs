//! Adapter between the PEX input protocol and SkateGM's portable host for the
//! recovered Skate 3 simulation. The host crate has no renderer; presentation
//! data crosses to Fabric as ordinary state payloads.

use crate::physics::Controls;
use skate_data::skate_map::SkateMap;
use skate_host::bridge::{self, Session};
use std::path::Path;
use std::{fs, io::ErrorKind};

const PROP_COLLISION_BOX_MAGIC: &[u8; 8] = b"PEXCOL1\0";
const PROP_COLLISION_MESH_MAGIC: &[u8; 8] = b"PEXCOL2\0";
const MAX_PROP_COLLISION_BOXES: usize = 8192;
const MAX_PROP_COLLISION_TRIANGLES: usize = 250_000;

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
    previous_respawn: bool,
    previous_shove: bool,
}

#[derive(Clone, Debug)]
pub struct SkateBones {
    pub hips: [f32; 3],
    pub present: u64,
    /// Quantized x/y/z offsets from hips, in 1/4096 Minecraft-block units.
    pub offsets: [[i16; 3]; 36],
    /// Quantized world-space bone rotations as x/y/z/w unit quaternions.
    pub rotations: [[i16; 4]; 36],
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
        session.set_difficulty(load_difficulty(asset_root)?);
        let initial_pose = session.activate(spawn, heading)?;
        Ok(Self {
            session,
            bone_names: initial_pose.names,
            bones_root_relative: None,
            previous_respawn: false,
            previous_shove: false,
        })
    }

    /// Load the authored retail map package produced from a user's own game
    /// data. The engine consumes its RWCM collision archive and native rails;
    /// map coordinates are in the same metre/block space as the PEX adapter.
    pub fn load_skate_map(asset_root: &Path, map_path: &Path) -> Result<Self, String> {
        let map = SkateMap::load(map_path)?;
        let spawn = map.spawn;
        let heading = map.heading;
        let mut session = Session::new_with_skate_map(asset_root, &map)?;
        session.set_difficulty(load_difficulty(asset_root)?);
        let prop_collision_path = authored_prop_collision_path(asset_root, map_path)?;
        match load_prop_collision(&prop_collision_path) {
            Ok(triangles) if !triangles.is_empty() => {
                let count = session.set_moving_collision(triangles)?;
                eprintln!("PEX_SKATE_PROP_COLLISION triangles={count}");
            }
            Ok(_) => {}
            Err(error) => {
                return Err(format!(
                    "could not load authored prop collision {}: {error}",
                    prop_collision_path.display()
                ));
            }
        }
        let initial_pose = session
            .activate(spawn, heading)
            .map_err(|error| format!("could not activate Skate map {:?}: {error}", map.name))?;
        Ok(Self {
            session,
            bone_names: initial_pose.names,
            bones_root_relative: None,
            previous_respawn: false,
            previous_shove: false,
        })
    }

    /// One deterministic 60 Hz engine step. The caller owns the server tick
    /// accumulator and must not call this directly from client packet cadence.
    pub fn tick(&mut self, controls: Controls) -> Result<SkatePose, String> {
        if pressed_edge(controls.respawn, self.previous_respawn) {
            if let Err(error) = self.session.return_to_checkpoint() {
                eprintln!("PEX Skate checkpoint respawn unavailable: {error}");
            }
        }
        self.previous_respawn = controls.respawn;
        if pressed_edge(controls.shove, self.previous_shove) {
            self.session.shove(18);
        }
        self.previous_shove = controls.shove;
        self.session.tick(to_engine_controls(controls))?;
        Ok(self.pose())
    }

    /// Start a shove immediately from a fresh server input edge. Returns
    /// whether the engine can play the action in the skater's current state.
    pub fn start_shove(&mut self) -> bool {
        self.previous_shove = true;
        self.session.shove(18)
    }

    /// Apply Skate 3's vehicle-ejection knockdown to this skater.
    pub fn knock_down(&mut self, velocity: [f32; 3]) -> bool {
        self.session.knock_down(velocity)
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
        let mut rotations = [[0_i16; 4]; 36];
        for (slot, name) in SKATER_BONE_NAMES.iter().enumerate() {
            let Some(index) = self.bone_names.iter().position(|bone| bone == name) else {
                continue;
            };
            let Some(matrix) = pose.bones.get(index) else {
                continue;
            };
            let world_matrix = if root_relative {
                pose.root * *matrix
            } else {
                *matrix
            };
            let world = world_matrix.to_cols_array();
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
            let (_, rotation, _) = world_matrix.to_scale_rotation_translation();
            rotations[slot] = quantize_rotation(rotation.to_array());
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
                rotations,
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

fn authored_prop_collision_path(
    asset_root: &Path,
    map_path: &Path,
) -> Result<std::path::PathBuf, String> {
    let filename = map_path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Skate map filename is invalid".to_owned())?;
    if filename.is_empty()
        || filename.contains("..")
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("Skate map filename is unsafe for prop collision lookup".into());
    }
    Ok(asset_root
        .join("private")
        .join("native-props")
        .join(format!("{filename}.collision")))
}

fn load_difficulty(asset_root: &Path) -> Result<u32, String> {
    let path = asset_root.join("settings").join("difficulty.txt");
    match fs::read_to_string(&path) {
        Ok(value) => {
            parse_difficulty(&value).map_err(|error| format!("{}: {error}", path.display()))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn parse_difficulty(value: &str) -> Result<u32, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "easy" | "" => Ok(0),
        "normal" => Ok(1),
        "hardcore" => Ok(2),
        _ => Err("expected easy, normal, or hardcore".into()),
    }
}

fn load_prop_collision(path: &Path) -> Result<Vec<[[f32; 3]; 3]>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    parse_prop_collision(&bytes)
}

fn parse_prop_collision(bytes: &[u8]) -> Result<Vec<[[f32; 3]; 3]>, String> {
    if bytes.len() < 12 {
        return Err("invalid PEX prop collision header".into());
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if &bytes[..8] == PROP_COLLISION_BOX_MAGIC {
        if count > MAX_PROP_COLLISION_BOXES || bytes.len() != 12 + count * 24 {
            return Err("invalid PEX prop collision box count or package length".into());
        }
        let mut triangles = Vec::with_capacity(count * 12);
        for record in bytes[12..].chunks_exact(24) {
            let mut values = [0.0f32; 6];
            for (index, bytes) in record.chunks_exact(4).enumerate() {
                values[index] = f32::from_le_bytes(bytes.try_into().unwrap());
            }
            let lower = [values[0], values[1], values[2]];
            let upper = [values[3], values[4], values[5]];
            if values.iter().any(|value| !value.is_finite())
                || (0..3).any(|axis| {
                    upper[axis] - lower[axis] < 0.04 || upper[axis] - lower[axis] > 128.0
                })
            {
                return Err(format!(
                    "invalid PEX prop collision bounds in box {}",
                    triangles.len() / 12
                ));
            }
            triangles.extend(box_triangles(lower, upper));
        }
        return Ok(triangles);
    }
    if &bytes[..8] != PROP_COLLISION_MESH_MAGIC
        || count > MAX_PROP_COLLISION_TRIANGLES
        || bytes.len() != 12 + count * 36
    {
        return Err("invalid PEX prop collision triangle count or package length".into());
    }
    let mut triangles = Vec::with_capacity(count);
    for record in bytes[12..].chunks_exact(36) {
        let mut triangle = [[0.0f32; 3]; 3];
        for (index, value_bytes) in record.chunks_exact(4).enumerate() {
            triangle[index / 3][index % 3] = f32::from_le_bytes(value_bytes.try_into().unwrap());
        }
        let [a, b, c] = triangle;
        if triangle.iter().flatten().any(|value| !value.is_finite())
            || triangle
                .iter()
                .any(|point| point.iter().any(|value| value.abs() > 1_000_000.0))
        {
            return Err(format!(
                "invalid PEX prop collision vertex in triangle {}",
                triangles.len()
            ));
        }
        let edge =
            |from: [f32; 3], to: [f32; 3]| [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let ab = edge(a, b);
        let ac = edge(a, c);
        let bc = edge(b, c);
        let length = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        if length(ab).max(length(ac)).max(length(bc)) > 128.0 || length(cross) <= 1.0e-6 {
            return Err(format!(
                "invalid PEX prop collision triangle {}",
                triangles.len()
            ));
        }
        triangles.push(triangle);
    }
    Ok(triangles)
}

fn box_triangles(min: [f32; 3], max: [f32; 3]) -> [[[f32; 3]; 3]; 12] {
    let p = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    [
        [p[0], p[3], p[2]],
        [p[0], p[2], p[1]],
        [p[4], p[5], p[6]],
        [p[4], p[6], p[7]],
        [p[0], p[4], p[7]],
        [p[0], p[7], p[3]],
        [p[1], p[2], p[6]],
        [p[1], p[6], p[5]],
        [p[0], p[1], p[5]],
        [p[0], p[5], p[4]],
        [p[3], p[7], p[6]],
        [p[3], p[6], p[2]],
    ]
}

fn pressed_edge(current: bool, previous: bool) -> bool {
    current && !previous
}

fn quantize_rotation(mut rotation: [f32; 4]) -> [i16; 4] {
    let magnitude = rotation
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if !magnitude.is_finite() || magnitude < 1.0e-5 {
        return [0, 0, 0, i16::MAX];
    }
    for value in &mut rotation {
        *value /= magnitude;
    }
    rotation.map(|value| (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
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
    const Y: u16 = 1 << 15;
    const LEFT_BUMPER: u16 = 1 << 8;
    let axis = |value: f32| (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
    bridge::Controls {
        buttons: u16::from(controls.dpad & 0x0f)
            | (if controls.push { A } else { 0 })
            | (if controls.brake { B } else { 0 })
            | (if controls.push_left { X } else { 0 })
            | (if controls.dismount { Y } else { 0 })
            | (if controls.session_modifier {
                LEFT_BUMPER
            } else {
                0
            }),
        triggers: controls.triggers,
        left: [axis(controls.steer), axis(controls.forward)],
        right: [axis(controls.right_x), axis(controls.right_y)],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        box_triangles, load_difficulty, load_prop_collision, parse_difficulty,
        parse_prop_collision, pressed_edge, quantize_rotation, to_engine_controls,
    };
    use crate::physics::Controls;

    #[test]
    fn input_axes_and_trick_actions_map_to_the_engine_pad_packet() {
        let input = to_engine_controls(Controls {
            forward: 1.0,
            steer: -0.5,
            right_x: -1.0,
            right_y: 1.0,
            triggers: [96, 255],
            dpad: 0,
            push: true,
            push_left: true,
            brake: true,
            dismount: false,
            session_modifier: false,
            respawn: false,
            shove: false,
        });
        assert_eq!(input.buttons, (1 << 12) | (1 << 13) | (1 << 14));
        assert_eq!(input.triggers, [96, 255]);
        assert_eq!(input.left[0], -16_384);
        assert_eq!(input.left[1], i16::MAX);
        assert_eq!(input.right, [-i16::MAX, i16::MAX]);
    }

    #[test]
    fn parses_supported_skate_physics_difficulties_case_insensitively() {
        assert_eq!(parse_difficulty("easy\n").unwrap(), 0);
        assert_eq!(parse_difficulty("Normal").unwrap(), 1);
        assert_eq!(parse_difficulty("HARDCORE").unwrap(), 2);
        assert!(parse_difficulty("motorized").is_err());
    }

    #[test]
    fn difficulty_setting_defaults_to_easy_and_reads_server_config() {
        let root = std::env::temp_dir().join(format!(
            "pex-skate-difficulty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(load_difficulty(&root).unwrap(), 0);
        std::fs::create_dir_all(root.join("settings")).unwrap();
        std::fs::write(root.join("settings/difficulty.txt"), "normal\n").unwrap();
        assert_eq!(load_difficulty(&root).unwrap(), 1);
        std::fs::write(root.join("settings/difficulty.txt"), "expert\n").unwrap();
        assert!(load_difficulty(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dpad_bits_reach_the_stock_gameplay_gesture_input() {
        let input = to_engine_controls(Controls {
            dpad: 0b1010,
            ..Controls::default()
        });
        assert_eq!(input.buttons & 0x0f, 0b1010);
    }

    #[test]
    fn on_off_board_input_maps_to_the_stock_y_button() {
        let input = to_engine_controls(Controls {
            dismount: true,
            ..Controls::default()
        });
        assert_eq!(input.buttons & (1 << 15), 1 << 15);
    }

    #[test]
    fn session_modifier_maps_to_the_stock_left_bumper_button() {
        let input = to_engine_controls(Controls {
            session_modifier: true,
            ..Controls::default()
        });
        assert_eq!(input.buttons & (1 << 8), 1 << 8);
    }

    #[test]
    fn checkpoint_respawn_fires_once_per_press_and_again_after_release() {
        let mut previous = false;
        let mut presses = 0;
        for current in [false, true, true, true, false, false, true, false] {
            if pressed_edge(current, previous) {
                presses += 1;
            }
            previous = current;
        }
        assert_eq!(presses, 2);
    }

    #[test]
    fn shove_fires_once_per_press_and_again_after_release() {
        let mut previous = false;
        let mut presses = 0;
        for current in [false, true, true, false, true, false] {
            if pressed_edge(current, previous) {
                presses += 1;
            }
            previous = current;
        }
        assert_eq!(presses, 2);
    }

    #[test]
    fn bone_rotation_quaternions_are_unit_quantized_and_invalid_values_fallback() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let rotation = quantize_rotation([0.0, 0.0, half, half]);
        assert_eq!(rotation, [0, 0, 23170, 23170]);
        assert_eq!(quantize_rotation([f32::NAN; 4]), [0, 0, 0, i16::MAX]);
    }

    #[test]
    fn prop_collision_package_expands_bounds_into_outward_facing_triangles() {
        let mut bytes = b"PEXCOL1\0".to_vec();
        bytes.extend(1u32.to_le_bytes());
        for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0] {
            bytes.extend(value.to_le_bytes());
        }
        let triangles = parse_prop_collision(&bytes).unwrap();
        assert_eq!(triangles.len(), 12);
        for triangle in triangles {
            let [a, b, c] = triangle;
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let center = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            let outward = [center[0] - 0.5, center[1] - 0.5, center[2] - 0.5];
            assert!(normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2] > 0.0);
        }
    }

    #[test]
    fn prop_collision_package_rejects_invalid_bounds_and_trailing_data() {
        let mut bytes = b"PEXCOL1\0".to_vec();
        bytes.extend(1u32.to_le_bytes());
        for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 0.01] {
            bytes.extend(value.to_le_bytes());
        }
        assert!(parse_prop_collision(&bytes).is_err());
        bytes.extend([0]);
        assert!(parse_prop_collision(&bytes).is_err());
        assert_eq!(box_triangles([0.0; 3], [1.0; 3]).len(), 12);
    }

    #[test]
    fn prop_collision_mesh_package_preserves_authored_triangles() {
        let triangle = [[0.0f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]];
        let mut bytes = b"PEXCOL2\0".to_vec();
        bytes.extend(1u32.to_le_bytes());
        for value in triangle.into_iter().flatten() {
            bytes.extend(value.to_le_bytes());
        }
        assert_eq!(parse_prop_collision(&bytes).unwrap(), vec![triangle]);
    }

    #[test]
    fn prop_collision_mesh_package_rejects_degenerate_and_oversized_triangles() {
        let mut bytes = b"PEXCOL2\0".to_vec();
        bytes.extend(1u32.to_le_bytes());
        for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        assert!(parse_prop_collision(&bytes).is_err());
        bytes.clear();
        bytes.extend(b"PEXCOL2\0");
        bytes.extend(1u32.to_le_bytes());
        for value in [0.0f32, 0.0, 0.0, 129.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        assert!(parse_prop_collision(&bytes).is_err());
    }

    #[test]
    fn missing_optional_prop_collision_file_is_empty() {
        let path = std::env::temp_dir().join(format!(
            "pex-skate-missing-props-{}-{}.collision",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!path.exists());
        assert!(load_prop_collision(&path).unwrap().is_empty());
    }

    #[test]
    fn malformed_optional_prop_collision_file_is_rejected() {
        let path = std::env::temp_dir().join(format!(
            "pex-skate-invalid-props-{}-{}.collision",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"broken").unwrap();
        let result = load_prop_collision(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(
            result
                .unwrap_err()
                .contains("invalid PEX prop collision header")
        );
    }
}
