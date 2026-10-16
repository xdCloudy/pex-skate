mod collision;
mod engine;
mod physics;

use collision::CollisionScan;
use engine::{SkatePose, SkateSession};
use physics::Controls;
use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Server,
    command::Command,
    commands::CommandHandler,
    events::{
        EventHandler, EventPriority,
        player::{PlayerCustomPayloadEvent, PlayerMoveEvent},
    },
    permissions,
    text::TextComponent,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static SKATE_STATE: OnceLock<Mutex<ServerState>> = OnceLock::new();
static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();
const INPUT_TIMEOUT: Duration = Duration::from_millis(250);
const STATE_BROADCAST_INTERVAL: Duration = Duration::from_millis(50);
const COLLISION_REFRESH_DISTANCE: f32 = 8.0;
const COLLISION_REFRESH_VERTICAL_DISTANCE: f32 = 3.0;
const COLLISION_BLOCKS_PER_MOVEMENT: usize = 1_024;

fn server_state() -> &'static Mutex<ServerState> {
    SKATE_STATE.get_or_init(|| Mutex::new(ServerState::default()))
}

#[derive(Default)]
struct ServerState {
    enabled: HashSet<String>,
    controls: HashMap<String, Controls>,
    control_updated_at: HashMap<String, Instant>,
    sessions: HashMap<String, SkateSession>,
    pending_loads: HashSet<String>,
    engine_accumulator: HashMap<String, f32>,
    engine_updated_at: HashMap<String, Instant>,
    last_broadcast_at: HashMap<String, Instant>,
    collision_center: HashMap<String, (f32, f32, f32)>,
    collision_scans: HashMap<String, CollisionScan>,
}

impl ServerState {
    fn controls_for(&self, name: &str, now: Instant) -> Controls {
        let fresh = self
            .control_updated_at
            .get(name)
            .is_some_and(|updated| now.saturating_duration_since(*updated) <= INPUT_TIMEOUT);
        if fresh {
            self.controls.get(name).copied().unwrap_or_default()
        } else {
            Controls::default()
        }
    }
}

struct PexSkate;

impl Plugin for PexSkate {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "pex-skate".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["PEX Skate contributors".into()],
            description:
                "A portable PEX adapter for the Skate 3 Rust Engine, with server-authoritative physics and a Fabric presentation client."
                    .into(),
            dependencies: vec![],
            permissions: vec![permissions::FS_READ_DATA.into()],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let data_root = PathBuf::from(context.get_data_folder());
        DATA_ROOT
            .set(data_root)
            .map_err(|_| "Skate data folder was initialized more than once".to_owned())?;
        let command = Command::new(&["skate".into()], "Toggle PEX Skate mode").execute(ToggleSkate);
        context.register_command(command, "");

        context
            .register_event_handler::<PlayerCustomPayloadEvent, _>(
                SkateInput,
                EventPriority::Normal,
                false,
            )
            .map_err(|error| format!("could not register client input handler: {error}"))?;
        context
            .register_event_handler::<PlayerMoveEvent, _>(
                SkateMovement,
                EventPriority::Normal,
                true,
            )
            .map_err(|error| format!("could not register movement handler: {error}"))?;
        context
            .register_event_handler::<pumpkin_plugin_api::events::player::PlayerLeaveEvent, _>(
                SkatePlayerLeave,
                EventPriority::Normal,
                false,
            )
            .map_err(|error| format!("could not register player cleanup handler: {error}"))?;
        Ok(())
    }
}

struct ToggleSkate;

impl CommandHandler for ToggleSkate {
    fn handle(
        &self,
        sender: pumpkin_plugin_api::command::CommandSender,
        server: Server,
        _args: pumpkin_plugin_api::command::ConsumedArgs,
    ) -> pumpkin_plugin_api::Result<i32, pumpkin_plugin_api::command::CommandError> {
        let name = sender.get_name();
        let should_disable = {
            let Ok(mut state) = server_state().lock() else {
                return Err(pumpkin_plugin_api::command::CommandError::CommandFailed(
                    TextComponent::text("Skate mode state is unavailable."),
                ));
            };
            if state.enabled.remove(&name) {
                state.controls.remove(&name);
                state.control_updated_at.remove(&name);
                state.sessions.remove(&name);
                state.pending_loads.remove(&name);
                state.engine_accumulator.remove(&name);
                state.engine_updated_at.remove(&name);
                state.last_broadcast_at.remove(&name);
                state.collision_center.remove(&name);
                state.collision_scans.remove(&name);
                true
            } else if !state.pending_loads.insert(name.clone()) {
                sender.send_message(TextComponent::text(
                    "Skate Engine is already loading for you.",
                ));
                return Ok(0);
            } else {
                false
            }
        };

        if should_disable {
            if let Some(player) = server.get_player_by_name(&name) {
                broadcast_state(
                    &server,
                    player.as_entity().get_id(),
                    player.get_position(),
                    player.get_yaw(),
                    0.0,
                    false,
                );
            }
            sender.send_message(TextComponent::text("PEX Skate mode disabled."));
        } else {
            let Some(player) = server.get_player_by_name(&name) else {
                clear_pending_load(&name);
                sender.send_message(TextComponent::text(
                    "Skate mode can only be enabled by a player.",
                ));
                return Ok(0);
            };
            let Some(data_root) = DATA_ROOT.get() else {
                clear_pending_load(&name);
                sender.send_message(TextComponent::text(
                    "Skate Engine data folder is unavailable.",
                ));
                return Ok(0);
            };
            let position = player.get_position();
            let collision =
                match collision::around_player(&player.as_entity().get_world(), position) {
                    Ok(collision) if !collision.triangles.is_empty() => collision,
                    Ok(_) => {
                        clear_pending_load(&name);
                        sender.send_message(TextComponent::text(
                            "No Minecraft collision was found near you.",
                        ));
                        return Ok(0);
                    }
                    Err(error) => {
                        clear_pending_load(&name);
                        sender.send_message(TextComponent::text(&format!(
                            "Could not read skate collision: {error}"
                        )));
                        return Ok(0);
                    }
                };
            let spawn = [position.0 as f32, position.1 as f32, position.2 as f32];
            let heading = player.get_yaw().to_radians();
            let session = match SkateSession::load(
                data_root,
                collision.triangles,
                collision.rails,
                spawn,
                heading,
            ) {
                Ok(session) => session,
                Err(error) => {
                    clear_pending_load(&name);
                    sender.send_message(TextComponent::text(&format!(
                        "Skate Engine could not load: {error}"
                    )));
                    return Ok(0);
                }
            };
            let mut session = session;
            let mut pose = session.pose();
            pose.visual = session.skeletal_pose();
            let now = Instant::now();
            let Ok(mut state) = server_state().lock() else {
                sender.send_message(TextComponent::text("Skate mode state is unavailable."));
                return Ok(0);
            };
            state.pending_loads.remove(&name);
            state.enabled.insert(name.clone());
            state.controls.insert(name.clone(), Controls::default());
            state.control_updated_at.insert(name.clone(), now);
            state.sessions.insert(name.clone(), session);
            state.engine_accumulator.insert(name.clone(), 0.0);
            state.engine_updated_at.insert(name.clone(), now);
            state.last_broadcast_at.insert(name.clone(), now);
            state
                .collision_center
                .insert(name.clone(), (spawn[0], spawn[1], spawn[2]));
            drop(state);
            broadcast_pose(&server, player.as_entity().get_id(), &pose, true);
            sender.send_message(TextComponent::text("Skate 3 Engine mode enabled."));
        }
        Ok(0)
    }
}

fn clear_pending_load(name: &str) {
    if let Ok(mut state) = server_state().lock() {
        state.pending_loads.remove(name);
    }
}

struct SkateInput;

impl EventHandler<PlayerCustomPayloadEvent> for SkateInput {
    fn handle(
        &self,
        _server: Server,
        event: pumpkin_plugin_api::events::EventData<PlayerCustomPayloadEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerCustomPayloadEvent> {
        if event.channel != "pexskate:control" {
            return event;
        }
        let Some(controls) = Controls::decode(&event.data) else {
            return event;
        };
        let name = event.player.get_name();
        if let Ok(mut state) = server_state().lock()
            && state.enabled.contains(&name)
        {
            state.controls.insert(name, controls);
            state
                .control_updated_at
                .insert(event.player.get_name(), Instant::now());
        }
        event
    }
}

struct SkatePlayerLeave;

impl EventHandler<pumpkin_plugin_api::events::player::PlayerLeaveEvent> for SkatePlayerLeave {
    fn handle(
        &self,
        server: Server,
        event: pumpkin_plugin_api::events::EventData<
            pumpkin_plugin_api::events::player::PlayerLeaveEvent,
        >,
    ) -> pumpkin_plugin_api::events::EventData<pumpkin_plugin_api::events::player::PlayerLeaveEvent>
    {
        let name = event.player.get_name();
        let entity_id = event.player.as_entity().get_id();
        let position = event.player.get_position();
        let yaw = event.player.get_yaw();
        if let Ok(mut state) = server_state().lock() {
            state.enabled.remove(&name);
            state.controls.remove(&name);
            state.control_updated_at.remove(&name);
            state.sessions.remove(&name);
            state.pending_loads.remove(&name);
            state.engine_accumulator.remove(&name);
            state.engine_updated_at.remove(&name);
            state.last_broadcast_at.remove(&name);
            state.collision_center.remove(&name);
            state.collision_scans.remove(&name);
        }
        broadcast_state(&server, entity_id, position, yaw, 0.0, false);
        event
    }
}

struct SkateMovement;

impl EventHandler<PlayerMoveEvent> for SkateMovement {
    fn handle(
        &self,
        server: Server,
        mut event: pumpkin_plugin_api::events::EventData<PlayerMoveEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerMoveEvent> {
        let name = event.player.get_name();
        let Ok(mut state) = server_state().lock() else {
            return event;
        };
        if !state.enabled.contains(&name) {
            return event;
        }

        let now = Instant::now();
        let controls = state.controls_for(&name, now);
        let previous = state
            .engine_updated_at
            .insert(name.clone(), now)
            .unwrap_or(now);
        let accumulated = state.engine_accumulator.entry(name.clone()).or_default();
        *accumulated += now
            .saturating_duration_since(previous)
            .as_secs_f32()
            .min(0.25);
        let mut remaining = *accumulated;
        let engine_result = if let Some(session) = state.sessions.get_mut(&name) {
            let period = session.period().clamp(1.0 / 120.0, 1.0 / 20.0);
            let mut pose = session.pose();
            let mut steps = 0;
            let mut failure = None;
            while remaining >= period && steps < 6 {
                match session.tick(controls) {
                    Ok(next_pose) => pose = next_pose,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
                remaining -= period;
                steps += 1;
            }
            if steps == 6 {
                remaining = remaining.min(period);
            }
            match failure {
                Some(error) => Err(error),
                None => Ok(pose),
            }
        } else {
            return event;
        };
        let mut pose = match engine_result {
            Ok(pose) => {
                state.engine_accumulator.insert(name.clone(), remaining);
                pose
            }
            Err(error) => {
                state.sessions.remove(&name);
                state.enabled.remove(&name);
                state.controls.remove(&name);
                state.control_updated_at.remove(&name);
                state.engine_accumulator.remove(&name);
                state.engine_updated_at.remove(&name);
                state.last_broadcast_at.remove(&name);
                state.collision_center.remove(&name);
                state.collision_scans.remove(&name);
                event.to_position = event.from_position;
                let position = event.from_position;
                let yaw = event.player.get_yaw();
                let entity_id = event.player.as_entity().get_id();
                drop(state);
                broadcast_state(&server, entity_id, position, yaw, 0.0, false);
                event.player.send_system_message(
                    TextComponent::text(&format!(
                        "Skate Engine stopped after a simulation error: {error}"
                    )),
                    true,
                );
                return event;
            }
        };
        event.to_position = (
            f64::from(pose.root[0]),
            f64::from(pose.root[1]),
            f64::from(pose.root[2]),
        );
        let center = state.collision_center.get(&name).copied().unwrap_or((
            pose.root[0],
            pose.root[1],
            pose.root[2],
        ));
        let should_refresh_collision = collision_refresh_needed(center, pose.root);
        let should_broadcast = state
            .last_broadcast_at
            .get(&name)
            .is_none_or(|last| now.saturating_duration_since(*last) >= STATE_BROADCAST_INTERVAL);
        if should_broadcast {
            state.last_broadcast_at.insert(name.clone(), now);
            if let Some(session) = state.sessions.get_mut(&name) {
                pose.visual = session.skeletal_pose();
            }
        }
        let entity_id = event.player.as_entity().get_id();
        drop(state);
        let world = event.player.as_entity().get_world();
        let scan_center = (
            f64::from(pose.root[0]),
            f64::from(pose.root[1]),
            f64::from(pose.root[2]),
        );
        if let Err(error) = advance_collision_scan(
            &world,
            &name,
            should_refresh_collision,
            scan_center,
            COLLISION_BLOCKS_PER_MOVEMENT,
        ) {
            stop_skate_after_collision_error(&server, &mut event, &name, entity_id, error);
            return event;
        }
        if should_broadcast {
            broadcast_pose(&server, entity_id, &pose, true);
        }
        event
    }
}

fn advance_collision_scan(
    world: &pumpkin_plugin_api::World,
    name: &str,
    start_if_needed: bool,
    requested_center: (f64, f64, f64),
    block_budget: usize,
) -> Result<(), String> {
    let mut scan = {
        let mut state = server_state()
            .lock()
            .map_err(|_| "Skate mode state is unavailable".to_owned())?;
        if start_if_needed {
            state
                .collision_scans
                .entry(name.to_owned())
                .or_insert_with(|| CollisionScan::new(requested_center));
        }
        state.collision_scans.remove(name)
    };
    let Some(mut scan) = scan.take() else {
        return Ok(());
    };
    if !scan.advance(world, block_budget)? {
        server_state()
            .lock()
            .map_err(|_| "Skate mode state is unavailable".to_owned())?
            .collision_scans
            .insert(name.to_owned(), scan);
        return Ok(());
    }

    let center = scan.center();
    let local = scan
        .into_local()
        .filter(|local| !local.triangles.is_empty())
        .ok_or_else(|| "Minecraft returned no collision at the new skate location".to_owned())?;
    let mut state = server_state()
        .lock()
        .map_err(|_| "Skate mode state is unavailable".to_owned())?;
    let Some(session) = state.sessions.get_mut(name) else {
        return Err("Skate session is no longer active".into());
    };
    session.refresh_collision(local.triangles, local.rails)?;
    state.collision_center.insert(name.to_owned(), center);
    Ok(())
}

fn stop_skate_after_collision_error(
    server: &Server,
    event: &mut pumpkin_plugin_api::events::EventData<PlayerMoveEvent>,
    name: &str,
    entity_id: u32,
    error: String,
) {
    if let Ok(mut state) = server_state().lock() {
        state.enabled.remove(name);
        state.sessions.remove(name);
        state.controls.remove(name);
        state.control_updated_at.remove(name);
        state.engine_accumulator.remove(name);
        state.engine_updated_at.remove(name);
        state.last_broadcast_at.remove(name);
        state.collision_center.remove(name);
        state.collision_scans.remove(name);
    }
    event.to_position = event.from_position;
    event.player.send_system_message(
        TextComponent::text(&format!(
            "Skate Engine stopped while updating local collision: {error}"
        )),
        true,
    );
    broadcast_state(
        server,
        entity_id,
        event.from_position,
        event.player.get_yaw(),
        0.0,
        false,
    );
}

fn collision_refresh_needed(center: (f32, f32, f32), root: [f32; 3]) -> bool {
    let dx = root[0] - center.0;
    let dy = root[1] - center.1;
    let dz = root[2] - center.2;
    dx * dx + dz * dz >= COLLISION_REFRESH_DISTANCE * COLLISION_REFRESH_DISTANCE
        || dy.abs() >= COLLISION_REFRESH_VERTICAL_DISTANCE
}

fn broadcast_pose(server: &Server, entity_id: u32, pose: &SkatePose, active: bool) {
    let mut payload = Vec::with_capacity(1100);
    payload.push(5); // version
    payload.push(u8::from(active));
    payload.extend_from_slice(&entity_id.to_be_bytes());
    for value in pose.root {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    for value in pose.board {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    for value in pose.velocity {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    for value in [
        pose.sequence_score,
        pose.line_score,
        pose.total_score,
        pose.multiplier,
        pose.line_time,
    ] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.push(u8::from(pose.clean) | (u8::from(pose.sketchy) << 1));
    let mut trick_name = pose.trick_name.clone();
    while trick_name.len() > 192 {
        trick_name.pop();
    }
    let trick_name = trick_name.as_bytes();
    payload.extend_from_slice(&(trick_name.len() as u16).to_be_bytes());
    payload.extend_from_slice(trick_name);
    append_visual(&mut payload, pose.visual.as_ref(), pose.root);
    broadcast_payload(server, &payload);
}

fn append_visual(
    payload: &mut Vec<u8>,
    visual: Option<&engine::SkateVisualPose>,
    fallback_hips: [f32; 3],
) {
    if let Some(bones) = visual.map(|visual| &visual.bones) {
        for value in bones.hips {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&bones.present.to_be_bytes());
        for offset in bones.offsets.iter().flatten() {
            payload.extend_from_slice(&offset.to_be_bytes());
        }
    } else {
        for value in fallback_hips {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&0_u64.to_be_bytes());
        payload.resize(payload.len() + 36 * 3 * 2, 0);
    }
    if let Some(camera) = visual.and_then(|visual| visual.camera.as_ref()) {
        for value in camera
            .offset
            .into_iter()
            .chain(camera.forward)
            .chain(camera.up)
            .chain([camera.fov])
        {
            payload.extend_from_slice(&value.to_be_bytes());
        }
    } else {
        payload.resize(payload.len() + 10 * 4, 0);
    }
    if let Some(bones) = visual.map(|visual| &visual.bones) {
        for rotation in bones.rotations.iter().flatten() {
            payload.extend_from_slice(&rotation.to_be_bytes());
        }
    } else {
        payload.resize(payload.len() + 36 * 4 * 2, 0);
    }
}

fn broadcast_state(
    server: &Server,
    entity_id: u32,
    position: (f64, f64, f64),
    yaw: f32,
    board_roll: f32,
    active: bool,
) {
    let mut payload = Vec::with_capacity(84);
    payload.push(4);
    payload.push(u8::from(active));
    payload.extend_from_slice(&entity_id.to_be_bytes());
    for value in [position.0 as f32, position.1 as f32, position.2 as f32] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    for value in [position.0 as f32, position.1 as f32, position.2 as f32] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    // Board quaternion (identity when disabling); the legacy arguments are
    // retained to keep the disable call site compact.
    let half_yaw = yaw.to_radians() * 0.5;
    let half_roll = board_roll * 0.5;
    let (sy, cy) = half_yaw.sin_cos();
    let (sr, cr) = half_roll.sin_cos();
    for value in [0.0_f32, sr * cy, sy * cr, cr * cy] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    for _ in 0..8 {
        payload.extend_from_slice(&0.0_f32.to_be_bytes());
    }
    payload.push(0); // clean / sketchy flags
    payload.extend_from_slice(&0_u16.to_be_bytes());
    for value in [position.0 as f32, position.1 as f32, position.2 as f32] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.extend_from_slice(&0_u64.to_be_bytes());
    payload.resize(payload.len() + 36 * 3 * 2, 0);
    payload.resize(payload.len() + 10 * 4, 0);

    broadcast_payload(server, &payload);
}

fn broadcast_payload(server: &Server, payload: &[u8]) {
    for viewer in server.get_all_players() {
        if let Some(java_player) = viewer.as_java() {
            java_player.send_custom_payload("pexskate:state", payload);
        }
    }
}

pumpkin_plugin_api::register_plugin!(PexSkate);

#[cfg(test)]
mod integration_tests {
    use super::{
        COLLISION_REFRESH_VERTICAL_DISTANCE, INPUT_TIMEOUT, ServerState, append_visual,
        collision_refresh_needed, engine, physics::Controls,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn stale_client_input_resets_to_neutral_controls() {
        let now = Instant::now();
        let mut state = ServerState::default();
        state.controls.insert(
            "rider".into(),
            Controls {
                push: true,
                forward: 1.0,
                ..Controls::default()
            },
        );
        state.control_updated_at.insert(
            "rider".into(),
            now - INPUT_TIMEOUT - Duration::from_millis(1),
        );
        assert_eq!(state.controls_for("rider", now), Controls::default());
    }

    #[test]
    fn recent_client_input_remains_active() {
        let now = Instant::now();
        let mut state = ServerState::default();
        let controls = Controls {
            push: true,
            forward: 1.0,
            ..Controls::default()
        };
        state.controls.insert("rider".into(), controls);
        state.control_updated_at.insert("rider".into(), now);
        assert_eq!(state.controls_for("rider", now), controls);
    }

    #[test]
    fn collision_refresh_covers_vertical_falls_before_leaving_sampled_depth() {
        let center = (12.0, 40.0, -5.0);
        assert!(!collision_refresh_needed(
            center,
            [
                12.0,
                40.0 + COLLISION_REFRESH_VERTICAL_DISTANCE - 0.01,
                -5.0
            ]
        ));
        assert!(collision_refresh_needed(
            center,
            [12.0, 40.0 - COLLISION_REFRESH_VERTICAL_DISTANCE, -5.0]
        ));
    }

    #[test]
    fn collision_refresh_keeps_horizontal_travel_threshold() {
        let center = (0.0, 64.0, 0.0);
        assert!(!collision_refresh_needed(center, [7.99, 64.0, 0.0]));
        assert!(collision_refresh_needed(center, [8.0, 64.0, 0.0]));
    }

    #[test]
    fn v5_visual_tail_has_fixed_skeleton_camera_and_rotation_layout() {
        let visual = engine::SkateVisualPose {
            bones: engine::SkateBones {
                hips: [4.0, 5.0, 6.0],
                present: 1 << 1,
                offsets: [[0; 3]; 36],
                rotations: [[0; 4]; 36],
            },
            camera: Some(engine::SkateCamera {
                offset: [1.25, 2.5, -3.75],
                forward: [0.0, 0.0, 1.0],
                up: [0.0, 1.0, 0.0],
                fov: 73.0,
            }),
        };
        let mut payload = Vec::new();
        append_visual(&mut payload, Some(&visual), [0.0; 3]);

        assert_eq!(payload.len(), 12 + 8 + 36 * 3 * 2 + 10 * 4 + 36 * 4 * 2);
        assert_eq!(&payload[0..4], &4.0_f32.to_be_bytes());
        assert_eq!(&payload[12..20], &(1_u64 << 1).to_be_bytes());
        assert_eq!(&payload[236..240], &1.25_f32.to_be_bytes());
        assert_eq!(&payload[272..276], &73.0_f32.to_be_bytes());
        assert_eq!(&payload[276..278], &0_i16.to_be_bytes());
    }
}
