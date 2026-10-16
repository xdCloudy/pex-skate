//! Local Minecraft collision export for the Skate 3 host. Geometry is sampled
//! only around a joining skater; no EA map data is required.

use pumpkin_plugin_api::{BlockState, World, wit::pumpkin::plugin::common::BlockPos};
use std::collections::HashMap;

const HORIZONTAL_RADIUS: i32 = 12;
const VERTICAL_BELOW: i32 = 4;
const VERTICAL_ABOVE: i32 = 8;
const MAX_TRIANGLES: usize = 120_000;

pub struct LocalCollision {
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub rails: Vec<Vec<[f32; 3]>>,
}

pub fn around_player(world: &World, position: (f64, f64, f64)) -> Result<LocalCollision, String> {
    let origin = (
        position.0.floor() as i32,
        position.1.floor() as i32,
        position.2.floor() as i32,
    );
    let min_x = origin.0 - HORIZONTAL_RADIUS;
    let max_x = origin.0 + HORIZONTAL_RADIUS;
    let min_z = origin.2 - HORIZONTAL_RADIUS;
    let max_z = origin.2 + HORIZONTAL_RADIUS;
    let min_y = origin.1 - VERTICAL_BELOW;
    let max_y = origin.1 + VERTICAL_ABOVE;

    let mut states = HashMap::new();
    let mut triangles = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                let state = world.get_block_state(BlockPos { x, y, z });
                if state.is_air || state.is_liquid || state.collision_shapes.is_empty() {
                    continue;
                }
                let mut grind_height = None;
                for bounds in &state.collision_shapes {
                    add_box(
                        &mut triangles,
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
                    if triangles.len() > MAX_TRIANGLES {
                        return Err("Minecraft collision sample exceeded the 120,000-triangle safety limit; use a smaller local skate area.".into());
                    }
                    grind_height = Some(
                        grind_height
                            .unwrap_or(f32::NEG_INFINITY)
                            .max(bounds.max.1 as f32),
                    );
                }
                if grindable(&state) {
                    if let Some(height) = grind_height {
                        states.insert((x, y, z), (state.block_name, height));
                    }
                }
            }
        }
    }

    let rails = rails_from_states(&states);
    Ok(LocalCollision { triangles, rails })
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
    use super::{add_box, rails_from_states};
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
}
