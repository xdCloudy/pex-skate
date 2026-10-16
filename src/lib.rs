mod collision;
mod engine;
mod physics;

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
    collision_center: HashMap<String, (f32, f32)>,
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
            pose.bones = session.skeletal_pose();
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
                .insert(name.clone(), (spawn[0], spawn[2]));
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
        let center = state
            .collision_center
            .get(&name)
            .copied()
            .unwrap_or((pose.root[0], pose.root[2]));
        let dx = pose.root[0] - center.0;
        let dz = pose.root[2] - center.1;
        let should_refresh_collision =
            dx * dx + dz * dz >= COLLISION_REFRESH_DISTANCE * COLLISION_REFRESH_DISTANCE;
        let should_broadcast = state
            .last_broadcast_at
            .get(&name)
            .is_none_or(|last| now.saturating_duration_since(*last) >= STATE_BROADCAST_INTERVAL);
        if should_broadcast {
            state.last_broadcast_at.insert(name.clone(), now);
            if let Some(session) = state.sessions.get_mut(&name) {
                pose.bones = session.skeletal_pose();
            }
        }
        let entity_id = event.player.as_entity().get_id();
        drop(state);
        if should_refresh_collision {
            let world = event.player.as_entity().get_world();
            let refreshed = collision::around_player(
                &world,
                (
                    f64::from(pose.root[0]),
                    f64::from(pose.root[1]),
                    f64::from(pose.root[2]),
                ),
            )
            .and_then(|local| {
                if local.triangles.is_empty() {
                    return Err("Minecraft returned no collision at the new skate location".into());
                }
                let mut state = server_state()
                    .lock()
                    .map_err(|_| "Skate mode state is unavailable".to_owned())?;
                let Some(session) = state.sessions.get_mut(&name) else {
                    return Err("Skate session is no longer active".into());
                };
                session.refresh_collision(local.triangles, local.rails)?;
                state
                    .collision_center
                    .insert(name.clone(), (pose.root[0], pose.root[2]));
                Ok(())
            });
            if let Err(error) = refreshed {
                if let Ok(mut state) = server_state().lock() {
                    state.enabled.remove(&name);
                    state.sessions.remove(&name);
                    state.controls.remove(&name);
                    state.control_updated_at.remove(&name);
                    state.engine_accumulator.remove(&name);
                    state.engine_updated_at.remove(&name);
                    state.last_broadcast_at.remove(&name);
                    state.collision_center.remove(&name);
                }
                event.to_position = event.from_position;
                broadcast_state(
                    &server,
                    entity_id,
                    event.from_position,
                    event.player.get_yaw(),
                    0.0,
                    false,
                );
                event.player.send_system_message(
                    TextComponent::text(&format!(
                        "Skate Engine stopped while updating local collision: {error}"
                    )),
                    true,
                );
                return event;
            }
        }
        if should_broadcast {
            broadcast_pose(&server, entity_id, &pose, true);
        }
        event
    }
}

fn broadcast_pose(server: &Server, entity_id: u32, pose: &SkatePose, active: bool) {
    let mut payload = Vec::with_capacity(400);
    payload.push(3); // version
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
    append_skeleton(&mut payload, pose.bones.as_ref(), pose.root);
    broadcast_payload(server, &payload);
}

fn append_skeleton(
    payload: &mut Vec<u8>,
    bones: Option<&engine::SkateBones>,
    fallback_hips: [f32; 3],
) {
    if let Some(bones) = bones {
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
    payload.push(3);
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
    use super::{INPUT_TIMEOUT, ServerState, physics::Controls};
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
}
