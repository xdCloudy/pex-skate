//! Shared, server-authoritative motion for converted DMO props.
//!
//! This is a bounded collision-response adapter around the engine's existing
//! moving-triangle layer. It intentionally keeps prop state separate from each
//! per-skater Skate session so every rider sees the same body transforms.

const BOARD_HALF_EXTENTS: [f32; 3] = [0.32, 0.16, 0.48];
const MIN_IMPACT_SPEED: f32 = 1.0;
const IMPULSE_SCALE: f32 = 0.28;
const MAX_PROP_SPEED: f32 = 7.0;
const MAX_PROP_ANGULAR_SPEED: f32 = 4.0;
const PROP_DENSITY: f32 = 2.0;
const MIN_PROP_MASS: f32 = 0.25;
const MAX_PROP_MASS: f32 = 500.0;
const LINEAR_DAMPING: f32 = 2.2;
const ANGULAR_DAMPING: f32 = 2.8;
pub const MAX_PROP_UPDATES: usize = 8_192;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropUpdate {
    pub identity: u64,
    pub translation: [f32; 3],
    pub yaw: f32,
    pub pivot: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct PropBody {
    pub identity: u64,
    pub transform: [f32; 16],
    local_bounds: [f32; 6],
    velocity: [f32; 3],
    translation: [f32; 3],
    angular_velocity: f32,
    initial_pivot: [f32; 3],
    yaw: f32,
    revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PropWorld {
    bodies: Vec<PropBody>,
    revision: u64,
}

impl PropWorld {
    pub fn new(bodies: Vec<PropBody>) -> Self {
        Self {
            bodies,
            revision: 0,
        }
    }

    pub fn transforms(&self) -> Vec<(u64, [f32; 16])> {
        self.bodies
            .iter()
            .map(|body| (body.identity, body.transform))
            .collect()
    }

    /// Return the latest transform for each body changed since `revision`.
    pub fn changes_since(&self, revision: u64) -> (Vec<PropUpdate>, u64) {
        let mut bodies = self
            .bodies
            .iter()
            .filter(|body| body.revision > revision)
            .collect::<Vec<_>>();
        bodies.sort_unstable_by_key(|body| body.revision);
        let count = bodies.len().min(MAX_PROP_UPDATES);
        let changes = bodies
            .iter()
            .take(count)
            .map(|body| PropUpdate {
                identity: body.identity,
                translation: body.translation,
                yaw: body.yaw,
                pivot: body.initial_pivot,
            })
            .collect();
        let acknowledged_revision = bodies
            .get(count.saturating_sub(1))
            .filter(|_| count > 0)
            .map_or(self.revision, |body| body.revision);
        (changes, acknowledged_revision)
    }

    /// Apply one swept board impact and advance existing body velocities.
    /// Returns whether any transform changed this step.
    pub fn step(
        &mut self,
        board_from: [f32; 3],
        board_to: [f32; 3],
        board_velocity: [f32; 3],
        delta_seconds: f32,
    ) -> bool {
        let dt = if delta_seconds.is_finite() {
            delta_seconds.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let valid_trajectory = board_from
            .into_iter()
            .chain(board_to)
            .chain(board_velocity)
            .all(f32::is_finite);
        let speed = length(board_velocity);
        if valid_trajectory && speed >= MIN_IMPACT_SPEED {
            let mut hit: Option<(usize, f32)> = None;
            for (index, body) in self.bodies.iter().enumerate() {
                let Some(bounds) = world_bounds(body) else {
                    continue;
                };
                if !point_inside_expanded_aabb(board_from, bounds) {
                    if let Some(time) = segment_aabb_hit_time(board_from, board_to, bounds)
                        && hit.is_none_or(|(_, closest)| time < closest)
                    {
                        hit = Some((index, time));
                    }
                }
            }
            if let Some((index, time)) = hit {
                let horizontal = [board_velocity[0], 0.0, board_velocity[2]];
                let horizontal_speed = length(horizontal);
                if horizontal_speed >= MIN_IMPACT_SPEED {
                    let direction = horizontal.map(|value| value / horizontal_speed);
                    let impulse = direction.map(|value| value * horizontal_speed * IMPULSE_SCALE);
                    let impact: [f32; 3] = std::array::from_fn(|axis| {
                        board_from[axis] + (board_to[axis] - board_from[axis]) * time
                    });
                    let body = &mut self.bodies[index];
                    let dimensions = body_dimensions(body);
                    let mass = estimated_mass(dimensions);
                    for axis in 0..3 {
                        body.velocity[axis] += impulse[axis] / mass;
                    }
                    let body_speed = length(body.velocity);
                    if body_speed > MAX_PROP_SPEED {
                        for component in &mut body.velocity {
                            *component *= MAX_PROP_SPEED / body_speed;
                        }
                    }
                    let pivot = body.current_pivot();
                    let lever_arm = [impact[0] - pivot[0], impact[2] - pivot[2]];
                    let torque_y = lever_arm[1] * impulse[0] - lever_arm[0] * impulse[2];
                    let inertia = (mass
                        * (dimensions[0] * dimensions[0] + dimensions[2] * dimensions[2])
                        / 12.0)
                        .clamp(0.1, MAX_PROP_MASS * 1_000.0);
                    body.angular_velocity = (body.angular_velocity + torque_y / inertia)
                        .clamp(-MAX_PROP_ANGULAR_SPEED, MAX_PROP_ANGULAR_SPEED);
                }
            }
        }

        let mut changed = false;
        let damping = (-LINEAR_DAMPING * dt).exp();
        let mut revision = self.revision;
        for body in &mut self.bodies {
            let mut body_changed = false;
            for axis in 0..3 {
                let offset = 12 + axis;
                let movement = body.velocity[axis] * dt;
                if movement.abs() > 1e-6 {
                    body.transform[offset] += movement;
                    body.translation[axis] += movement;
                    changed = true;
                    body_changed = true;
                }
                body.velocity[axis] *= damping;
                if body.velocity[axis].abs() < 0.01 {
                    body.velocity[axis] = 0.0;
                }
            }
            body.angular_velocity *= (-ANGULAR_DAMPING * dt).exp();
            if body.angular_velocity.abs() < 0.01 {
                body.angular_velocity = 0.0;
            }
            let yaw_delta = body.angular_velocity * dt;
            if yaw_delta.abs() > 1e-6 {
                let pivot = body.current_pivot();
                rotate_about_pivot(&mut body.transform, pivot, yaw_delta);
                body.yaw = (body.yaw + yaw_delta).rem_euclid(TAU);
                changed = true;
                body_changed = true;
            }
            if body_changed {
                revision = revision.wrapping_add(1);
                body.revision = revision;
            }
        }
        self.revision = revision;
        changed
    }
}

/// Estimate world-space local dimensions, preserving scale without letting
/// current yaw rotation change the body's mass or inertia from frame to frame.
fn body_dimensions(body: &PropBody) -> [f32; 3] {
    std::array::from_fn(|axis| {
        let extent = (body.local_bounds[axis + 3] - body.local_bounds[axis]).abs();
        let column = [
            body.transform[axis * 4],
            body.transform[axis * 4 + 1],
            body.transform[axis * 4 + 2],
        ];
        let dimension = extent * length(column);
        if dimension.is_finite() {
            dimension
        } else {
            0.0
        }
    })
}

/// Use uniform density as a fallback because the converted DMO data has no
/// dependable authored mass. The bounds-derived estimate remains explicit and
/// capped so malformed or enormous scenery cannot destabilize the simulation.
fn estimated_mass(dimensions: [f32; 3]) -> f32 {
    let volume = dimensions.into_iter().product::<f32>();
    (volume * PROP_DENSITY)
        .clamp(MIN_PROP_MASS, MAX_PROP_MASS)
        .max(MIN_PROP_MASS)
}

impl PropBody {
    pub fn new(identity: u64, transform: [f32; 16], local_bounds: [f32; 6]) -> Self {
        let initial_origin = [transform[12], transform[13], transform[14]];
        let local_pivot =
            std::array::from_fn(|axis| (local_bounds[axis] + local_bounds[axis + 3]) * 0.5);
        let initial_pivot = transform_point(local_pivot, &transform).unwrap_or(initial_origin);
        Self {
            identity,
            transform,
            local_bounds,
            velocity: [0.0; 3],
            translation: [0.0; 3],
            angular_velocity: 0.0,
            initial_pivot,
            yaw: 0.0,
            revision: 0,
        }
    }

    fn current_pivot(&self) -> [f32; 3] {
        std::array::from_fn(|axis| self.initial_pivot[axis] + self.translation[axis])
    }
}

fn rotate_about_pivot(matrix: &mut [f32; 16], pivot: [f32; 3], angle: f32) {
    let (sin, cos) = angle.sin_cos();
    let old_x = [matrix[0], matrix[4], matrix[8]];
    let old_z = [matrix[2], matrix[6], matrix[10]];
    for column in 0..3 {
        matrix[column * 4] = cos * old_x[column] + sin * old_z[column];
        matrix[column * 4 + 2] = -sin * old_x[column] + cos * old_z[column];
    }
    let relative = [matrix[12] - pivot[0], matrix[14] - pivot[2]];
    matrix[12] = pivot[0] + cos * relative[0] + sin * relative[1];
    matrix[14] = pivot[2] - sin * relative[0] + cos * relative[1];
}

fn world_bounds(body: &PropBody) -> Option<[f32; 6]> {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for mask in 0..8 {
        let point = std::array::from_fn(|axis| {
            let low = body.local_bounds[axis];
            let high = body.local_bounds[axis + 3];
            if mask & (1 << axis) == 0 { low } else { high }
        });
        let world = transform_point(point, &body.transform)?;
        for axis in 0..3 {
            bounds[axis] = bounds[axis].min(world[axis]);
            bounds[axis + 3] = bounds[axis + 3].max(world[axis]);
        }
    }
    Some(bounds)
}

fn transform_point(point: [f32; 3], matrix: &[f32; 16]) -> Option<[f32; 3]> {
    let transformed = [
        point[0] * matrix[0] + point[1] * matrix[4] + point[2] * matrix[8] + matrix[12],
        point[0] * matrix[1] + point[1] * matrix[5] + point[2] * matrix[9] + matrix[13],
        point[0] * matrix[2] + point[1] * matrix[6] + point[2] * matrix[10] + matrix[14],
    ];
    transformed
        .iter()
        .all(|value| value.is_finite())
        .then_some(transformed)
}

fn point_inside_expanded_aabb(point: [f32; 3], bounds: [f32; 6]) -> bool {
    (0..3).all(|axis| {
        point[axis] >= bounds[axis] - BOARD_HALF_EXTENTS[axis]
            && point[axis] <= bounds[axis + 3] + BOARD_HALF_EXTENTS[axis]
    })
}

fn segment_aabb_hit_time(from: [f32; 3], to: [f32; 3], bounds: [f32; 6]) -> Option<f32> {
    let mut near: f32 = 0.0;
    let mut far: f32 = 1.0;
    for axis in 0..3 {
        let minimum = bounds[axis] - BOARD_HALF_EXTENTS[axis];
        let maximum = bounds[axis + 3] + BOARD_HALF_EXTENTS[axis];
        let delta = to[axis] - from[axis];
        if delta.abs() < 1e-7 {
            if from[axis] < minimum || from[axis] > maximum {
                return None;
            }
            continue;
        }
        let first = (minimum - from[axis]) / delta;
        let second = (maximum - from[axis]) / delta;
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return None;
        }
    }
    (far >= 0.0 && near <= 1.0).then_some(near.max(0.0))
}

fn length(vector: [f32; 3]) -> f32 {
    vector.iter().map(|value| value * value).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::{MAX_PROP_UPDATES, PropBody, PropWorld};

    fn identity() -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    fn box_body() -> PropBody {
        PropBody::new(0x1234, identity(), [-0.5, -0.5, -0.5, 0.5, 0.5, 0.5])
    }

    #[test]
    fn fast_board_sweep_moves_one_prop_and_publishes_its_transform() {
        let mut world = PropWorld::new(vec![box_body()]);
        assert!(world.step([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], [8.0, 0.0, 0.0], 0.05));
        let (updates, revision) = world.changes_since(0);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].identity, 0x1234);
        assert!(updates[0].translation[0] > 0.0);
        assert!(world.changes_since(revision).0.is_empty());
        let transforms = world.transforms();
        assert_eq!(transforms[0].0, 0x1234);
        assert!(transforms[0].1[12] > 0.0);
    }

    #[test]
    fn swept_board_hits_nearest_prop_even_when_body_order_is_reversed() {
        let mut farther = box_body();
        farther.identity = 2;
        farther.transform[12] = 4.0;
        let nearer = box_body();
        let mut world = PropWorld::new(vec![farther, nearer]);
        world.step([-3.0, 0.0, 0.0], [7.0, 0.0, 0.0], [8.0, 0.0, 0.0], 0.05);
        let transforms = world.transforms();
        let farther = transforms.iter().find(|(id, _)| *id == 2).unwrap();
        let nearer = transforms.iter().find(|(id, _)| *id == 0x1234).unwrap();
        assert!((farther.1[12] - 4.0).abs() < 1e-6);
        assert!(nearer.1[12] > 0.0);
    }

    #[test]
    fn off_center_board_impact_rotates_prop_about_its_collision_pivot() {
        let mut world = PropWorld::new(vec![box_body()]);
        assert!(world.step([-3.0, 0.0, 0.35], [3.0, 0.0, 0.35], [8.0, 0.0, 0.0], 0.05));
        let (updates, _) = world.changes_since(0);
        assert_eq!(updates.len(), 1);
        assert!(updates[0].yaw > 0.0);
        assert!(world.transforms()[0].1[0] < 1.0);
        assert!(world.transforms()[0].1[2] < 0.0);
        let body = &world.bodies[0];
        let pivot_from_transform = super::transform_point([0.0; 3], &body.transform).unwrap();
        let expected_pivot = body.current_pivot();
        for axis in 0..3 {
            assert!((pivot_from_transform[axis] - expected_pivot[axis]).abs() < 1e-5);
        }
    }

    #[test]
    fn collision_bounds_scale_linear_and_angular_response() {
        let small = PropBody::new(1, identity(), [-0.5, -0.5, -0.5, 0.5, 0.5, 0.5]);
        let large = PropBody::new(2, identity(), [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        let mut small_world = PropWorld::new(vec![small]);
        let mut large_world = PropWorld::new(vec![large]);

        for world in [&mut small_world, &mut large_world] {
            world.step([-3.0, 0.0, 0.4], [3.0, 0.0, 0.4], [8.0, 0.0, 0.0], 0.05);
        }

        let small_update = small_world.changes_since(0).0[0];
        let large_update = large_world.changes_since(0).0[0];
        assert!(small_update.translation[0] > large_update.translation[0]);
        assert!(small_update.yaw > large_update.yaw);
    }

    #[test]
    fn stationary_or_vertical_motion_does_not_push_props() {
        let mut stationary = PropWorld::new(vec![box_body()]);
        assert!(!stationary.step([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0; 3], 0.05));
        let mut vertical = PropWorld::new(vec![box_body()]);
        assert!(!vertical.step([0.0, -3.0, 0.0], [0.0, 3.0, 0.0], [0.0, 8.0, 0.0], 0.05));
        assert_eq!(vertical.transforms()[0].1, identity());
    }

    #[test]
    fn prop_motion_damps_and_rejects_nonfinite_time() {
        let mut world = PropWorld::new(vec![box_body()]);
        world.step([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], [8.0, 0.0, 0.0], 0.05);
        let after_impact = world.transforms()[0].1[12];
        assert!(!world.step([3.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0; 3], f32::NAN));
        assert!(world.step([3.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0; 3], 0.05));
        assert!(world.transforms()[0].1[12] > after_impact);
    }

    #[test]
    fn oversized_prop_snapshots_resume_after_the_last_transmitted_revision() {
        let mut bodies = (0..=MAX_PROP_UPDATES)
            .map(|identity| box_body_with_identity(identity as u64))
            .collect::<Vec<_>>();
        for (index, body) in bodies.iter_mut().enumerate() {
            body.revision = index as u64 + 1;
        }
        let world = PropWorld {
            bodies,
            revision: MAX_PROP_UPDATES as u64 + 1,
        };

        let (updates, acknowledged) = world.changes_since(0);
        assert_eq!(updates.len(), MAX_PROP_UPDATES);
        assert_eq!(updates[0].identity, 0);
        assert_eq!(
            updates[MAX_PROP_UPDATES - 1].identity,
            MAX_PROP_UPDATES as u64 - 1
        );
        assert_eq!(acknowledged, MAX_PROP_UPDATES as u64);

        let (remaining, final_revision) = world.changes_since(acknowledged);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].identity, MAX_PROP_UPDATES as u64);
        assert_eq!(final_revision, MAX_PROP_UPDATES as u64 + 1);
    }

    fn box_body_with_identity(id: u64) -> PropBody {
        PropBody::new(id, identity(), [-0.5, -0.5, -0.5, 0.5, 0.5, 0.5])
    }
}
