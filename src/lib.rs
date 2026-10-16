mod physics;

use physics::{Controls, SkateState};
use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Server,
    command::Command,
    commands::CommandHandler,
    events::{
        EventHandler, EventPriority,
        player::{PlayerCustomPayloadEvent, PlayerMoveEvent},
    },
    text::TextComponent,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static SKATE_STATE: OnceLock<Mutex<ServerState>> = OnceLock::new();
const INPUT_TIMEOUT: Duration = Duration::from_millis(250);
const STATE_BROADCAST_INTERVAL: Duration = Duration::from_millis(50);

fn server_state() -> &'static Mutex<ServerState> {
    SKATE_STATE.get_or_init(|| Mutex::new(ServerState::default()))
}

#[derive(Default)]
struct ServerState {
    enabled: HashSet<String>,
    controls: HashMap<String, Controls>,
    control_updated_at: HashMap<String, Instant>,
    physics: HashMap<String, SkateState>,
    last_broadcast_at: HashMap<String, Instant>,
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
                "A portable PEX skate-mode server plugin with a versioned client input protocol."
                    .into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
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
        let Ok(mut state) = server_state().lock() else {
            return Err(pumpkin_plugin_api::command::CommandError::CommandFailed(
                TextComponent::text("Skate mode state is unavailable."),
            ));
        };

        if state.enabled.remove(&name) {
            state.controls.remove(&name);
            state.control_updated_at.remove(&name);
            state.physics.remove(&name);
            state.last_broadcast_at.remove(&name);
            drop(state);
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
            state.enabled.insert(name.clone());
            state.controls.insert(name.clone(), Controls::default());
            state.control_updated_at.insert(name.clone(), Instant::now());
            state.physics.insert(name.clone(), SkateState::default());
            drop(state);
            if let Some(player) = server.get_player_by_name(&name) {
                broadcast_state(
                    &server,
                    player.as_entity().get_id(),
                    player.get_position(),
                    player.get_yaw(),
                    0.0,
                    true,
                );
            }
            sender.send_message(TextComponent::text(
                "PEX Skate mode enabled. Use the PEX Skate client companion for board controls, camera, and visuals.",
            ));
        }
        Ok(0)
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
            state.control_updated_at
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
    ) -> pumpkin_plugin_api::events::EventData<
        pumpkin_plugin_api::events::player::PlayerLeaveEvent,
    > {
        let name = event.player.get_name();
        let entity_id = event.player.as_entity().get_id();
        let position = event.player.get_position();
        let yaw = event.player.get_yaw();
        if let Ok(mut state) = server_state().lock() {
            state.enabled.remove(&name);
            state.controls.remove(&name);
            state.control_updated_at.remove(&name);
            state.physics.remove(&name);
            state.last_broadcast_at.remove(&name);
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
        let skate = state.physics.entry(name.clone()).or_default();
        let result = skate.tick(
            controls,
            event.player.get_yaw(),
            event.player.as_entity().is_on_ground(),
        );
        let from = event.from_position;
        let requested = event.to_position;

        // Preserve Minecraft's own vertical collision/gravity response. The
        // server-owned skate controller replaces only horizontal displacement.
        let mut y = requested.1;
        if result.jumped {
            y = from.1 + result.velocity_y;
        }
        event.to_position = (from.0 + result.velocity_x, y, from.2 + result.velocity_z);
        let roll = skate.board_roll;
        let board_yaw = skate.yaw.to_degrees();
        let total_score = skate.score;
        let landed_score = result.landed_score;
        let position = event.to_position;
        let entity_id = event.player.as_entity().get_id();
        let should_broadcast = state
            .last_broadcast_at
            .get(&name)
            .is_none_or(|last| now.saturating_duration_since(*last) >= STATE_BROADCAST_INTERVAL);
        if should_broadcast {
            state.last_broadcast_at.insert(name, now);
        }
        drop(state);
        if let Some(points) = landed_score {
            event.player.send_system_message(
                TextComponent::text(&format!("Landed combo: {points} points (total: {total_score}).")),
                true,
            );
        }
        if should_broadcast {
            broadcast_state(&server, entity_id, position, board_yaw, roll, true);
        }
        event
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
    let mut payload = Vec::with_capacity(38);
    payload.push(1);
    payload.push(u8::from(active));
    payload.extend_from_slice(&entity_id.to_be_bytes());
    payload.extend_from_slice(&position.0.to_be_bytes());
    payload.extend_from_slice(&position.1.to_be_bytes());
    payload.extend_from_slice(&position.2.to_be_bytes());
    payload.extend_from_slice(&yaw.to_be_bytes());
    payload.extend_from_slice(&board_roll.to_be_bytes());

    for viewer in server.get_all_players() {
        if let Some(java_player) = viewer.as_java() {
            java_player.send_custom_payload("pexskate:state", &payload);
        }
    }
}

pumpkin_plugin_api::register_plugin!(PexSkate);

#[cfg(test)]
mod integration_tests {
    use super::{INPUT_TIMEOUT, ServerState, physics::{Controls, SkateState}};
    use std::time::{Duration, Instant};

    #[test]
    fn stale_client_input_resets_to_neutral_controls() {
        let now = Instant::now();
        let mut state = ServerState::default();
        state.controls.insert(
            "rider".into(),
            Controls { push: true, forward: 1.0, ..Controls::default() },
        );
        state.control_updated_at.insert("rider".into(), now - INPUT_TIMEOUT - Duration::from_millis(1));
        assert_eq!(state.controls_for("rider", now), Controls::default());
    }

    #[test]
    fn recent_client_input_remains_active() {
        let now = Instant::now();
        let mut state = ServerState::default();
        let controls = Controls { push: true, forward: 1.0, ..Controls::default() };
        state.controls.insert("rider".into(), controls);
        state.control_updated_at.insert("rider".into(), now);
        assert_eq!(state.controls_for("rider", now), controls);
    }

    #[test]
    fn server_skate_state_is_bounded_at_high_speed() {
        let mut state = SkateState::default();
        for _ in 0..500 {
            state.tick(
                Controls {
                    forward: 1.0,
                    push: true,
                    ..Controls::default()
                },
                0.0,
                true,
            );
        }
        assert!(state.speed() <= 0.780_001);
    }
}
