//! Local Minecraft collision export for the Skate 3 host. Geometry is sampled
//! only around a joining skater; no EA map data is required.

use pumpkin_plugin_api::{BlockState, World, wit::pumpkin::plugin::common::BlockPos};
use std::collections::HashMap;

const HORIZONTAL_RADIUS: i32 = 12;
const VERTICAL_BELOW: i32 = 4;
const VERTICAL_ABOVE: i32 = 8;
const MAX_TRIANGLES: usize = 120_000;
const SAMPLE_WIDTH: usize = (HORIZONTAL_RADIUS * 2 + 1) as usize;
const SAMPLE_HEIGHT: usize = (VERTICAL_BELOW + VERTICAL_ABOVE + 1) as usize;
const SAMPLE_BLOCKS: usize = SAMPLE_WIDTH * SAMPLE_WIDTH * SAMPLE_HEIGHT;

pub struct LocalCollision {
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub rails: Vec<Vec<[f32; 3]>>,
}

/// A resumable local collision scan. `advance` bounds the number of host world
/// queries performed during one server tick callback.
pub struct CollisionScan {
    origin: (i32, i32, i32),
    center: (f32, f32, f32),
    cursor: usize,
    grindable_states: HashMap<(i32, i32, i32), (String, f32)>,
    block_state_cache: HashMap<u16, Option<BlockState>>,
    triangles: Vec<[[f32; 3]; 3]>,
}

impl CollisionScan {
    pub fn new(position: (f64, f64, f64)) -> Self {
        let origin = (
            position.0.floor() as i32,
            position.1.floor() as i32,
            position.2.floor() as i32,
        );
        Self {
            origin,
            center: (position.0 as f32, position.1 as f32, position.2 as f32),
            cursor: 0,
            grindable_states: HashMap::new(),
            block_state_cache: HashMap::new(),
            triangles: Vec::new(),
        }
    }

    pub fn center(&self) -> (f32, f32, f32) {
        self.center
    }

    pub fn is_complete(&self) -> bool {
        self.cursor == SAMPLE_BLOCKS
    }

    pub fn advance(&mut self, world: &World, budget: usize) -> Result<bool, String> {
        let end = sample_batch_end(self.cursor, budget);
        while self.cursor < end {
            let (x, y, z) = sample_position(self.origin, self.cursor);
            self.cursor += 1;
            let position = BlockPos { x, y, z };
            let state_id = world.get_block_state_id(position);
            let Some(state) = cached_lookup(&mut self.block_state_cache, state_id, || {
                BlockState::from_id(state_id)
            }) else {
                continue;
            };
            if state.is_air || state.is_liquid || state.collision_shapes.is_empty() {
                continue;
            }
            let mut grind_height = None;
            for bounds in &state.collision_shapes {
                add_box(
                    &mut self.triangles,
                    [
                        x as f32 + bounds.min.0 as f32,
                        y as f32 + bounds.min.1 as f32,
                        z as f32 + bounds.min.2 as f32,
                    ],
                    [
                        x as f32 + bounds.max.0 as f32,
                        y as f32 + bounds.max.1 as f32,
                        z as f32 + bounds.max.2 as f32,
                    ],
                );
                if self.triangles.len() > MAX_TRIANGLES {
                    return Err("Minecraft collision sample exceeded the 120,000-triangle safety limit; use a smaller local skate area.".into());
                }
                grind_height = Some(
                    grind_height
                        .unwrap_or(f32::NEG_INFINITY)
                        .max(bounds.max.1 as f32),
                );
            }
            if grindable(state) {
                if let Some(height) = grind_height {
                    self.grindable_states
                        .insert((x, y, z), (state.block_name.clone(), height));
                }
            }
        }
        Ok(self.is_complete())
    }

    pub fn into_local(self) -> Option<LocalCollision> {
        self.is_complete().then(|| LocalCollision {
            triangles: self.triangles,
            rails: rails_from_states(&self.grindable_states),
        })
    }
}

fn sample_position(origin: (i32, i32, i32), index: usize) -> (i32, i32, i32) {
    let x_offset = ((index / SAMPLE_WIDTH) % SAMPLE_WIDTH) as i32;
    let y_offset = (index / (SAMPLE_WIDTH * SAMPLE_WIDTH)) as i32;
    let z_offset = (index % SAMPLE_WIDTH) as i32;
    (
        origin.0 - HORIZONTAL_RADIUS + x_offset,
        origin.1 - VERTICAL_BELOW + y_offset,
        origin.2 - HORIZONTAL_RADIUS + z_offset,
    )
}

fn sample_batch_end(cursor: usize, budget: usize) -> usize {
    cursor.saturating_add(budget).min(SAMPLE_BLOCKS)
}

fn cached_lookup<T>(
    cache: &mut HashMap<u16, Option<T>>,
    state_id: u16,
    lookup: impl FnOnce() -> Option<T>,
) -> Option<&T> {
    cache.entry(state_id).or_insert_with(lookup).as_ref()
}

fn grindable(state: &BlockState) -> bool {
    let name = state.block_name.as_str();
    name.ends_with("_bars")
        || name.ends_with("_fence")
        || name.ends_with("_wall")
        || name.ends_with("_chain")
}

fn rails_from_states(states: &HashMap<(i32, i32, i32), (String, f32)>) -> Vec<Vec<[f32; 3]>> {
    let mut rails = Vec::new();
    for (&(x, y, z), (name, top)) in states {
        let start = [x as f32 + 0.5, y as f32 + top, z as f32 + 0.5];
        for (dx, dz) in [(1, 0), (0, 1)] {
            if states
                .get(&(x + dx, y, z + dz))
                .is_some_and(|(neighbor, _)| neighbor == name)
            {
                rails.push(vec![
                    start,
                    [start[0] + dx as f32, start[1], start[2] + dz as f32],
                ]);
            }
        }
    }
    rails
}

fn add_box(out: &mut Vec<[[f32; 3]; 3]>, min: [f32; 3], max: [f32; 3]) {
    let v = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    const FACES: [[usize; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 4, 7, 3],
        [1, 2, 6, 5],
        [0, 1, 5, 4],
        [3, 7, 6, 2],
    ];
    for [a, b, c, d] in FACES {
        out.push([v[a], v[b], v[c]]);
        out.push([v[a], v[c], v[d]]);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SAMPLE_BLOCKS, SAMPLE_HEIGHT, SAMPLE_WIDTH, add_box, cached_lookup, rails_from_states,
        sample_batch_end, sample_position,
    };
    use std::cell::Cell;
    use std::collections::HashMap;

    #[test]
    fn collision_boxes_export_twelve_triangles_with_expected_bounds() {
        let mut triangles = Vec::new();
        add_box(&mut triangles, [2.0, 3.0, 4.0], [3.0, 4.0, 5.0]);
        assert_eq!(triangles.len(), 12);
        assert!(triangles.iter().flatten().flatten().all(|v| v.is_finite()));
        assert!(
            triangles
                .iter()
                .flatten()
                .all(|p| (2.0..=5.0).contains(&p[0])
                    && (3.0..=4.0).contains(&p[1])
                    && (4.0..=5.0).contains(&p[2]))
        );
    }

    #[test]
    fn connected_grindable_blocks_become_rail_segments() {
        let states = HashMap::from([
            ((0, 1, 0), ("minecraft:iron_bars".to_owned(), 1.5)),
            ((1, 1, 0), ("minecraft:iron_bars".to_owned(), 1.5)),
            ((2, 1, 0), ("minecraft:iron_bars".to_owned(), 1.5)),
        ]);
        let rails = rails_from_states(&states);
        assert_eq!(rails.len(), 2);
        assert!(rails.iter().all(|rail| rail.len() == 2));
        assert!(rails.iter().flatten().all(|point| point[1] == 2.5));
    }

    #[test]
    fn detailed_block_state_lookup_is_cached_by_state_id() {
        let mut cache = HashMap::new();
        let calls = Cell::new(0);
        for _ in 0..3 {
            let state = cached_lookup(&mut cache, 42, || {
                calls.set(calls.get() + 1);
                Some("minecraft:stone")
            });
            assert_eq!(state, Some(&"minecraft:stone"));
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn missing_block_state_id_is_cached_as_well() {
        let mut cache = HashMap::new();
        let calls = Cell::new(0);
        for _ in 0..3 {
            assert_eq!(
                cached_lookup(&mut cache, 65_535, || {
                    calls.set(calls.get() + 1);
                    None::<()>
                }),
                None
            );
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn sample_positions_cover_the_volume_in_original_y_x_z_order() {
        let origin = (100, 64, -30);
        assert_eq!(SAMPLE_BLOCKS, SAMPLE_WIDTH * SAMPLE_WIDTH * SAMPLE_HEIGHT);
        assert_eq!(sample_position(origin, 0), (88, 60, -42));
        assert_eq!(sample_position(origin, SAMPLE_WIDTH - 1), (88, 60, -18));
        assert_eq!(sample_position(origin, SAMPLE_WIDTH), (89, 60, -42));
        assert_eq!(
            sample_position(origin, SAMPLE_WIDTH * SAMPLE_WIDTH),
            (88, 61, -42)
        );
        assert_eq!(sample_position(origin, SAMPLE_BLOCKS - 1), (112, 72, -18));
    }

    #[test]
    fn collision_scan_batches_never_exceed_the_requested_budget() {
        assert_eq!(sample_batch_end(0, 1_024), 1_024);
        assert_eq!(sample_batch_end(4_000, 1_024) - 4_000, 1_024);
        assert_eq!(sample_batch_end(SAMPLE_BLOCKS - 12, 1_024), SAMPLE_BLOCKS);
        assert_eq!(sample_batch_end(SAMPLE_BLOCKS, 1_024), SAMPLE_BLOCKS);
    }
}
