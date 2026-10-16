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
        entity::PlayerDeathEvent,
        player::{
            PlayerChangedWorldEvent, PlayerCustomPayloadEvent, PlayerMoveEvent, PlayerRespawnEvent,
        },
        server::ServerTickEndEvent,
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
const COLLISION_BLOCKS_PER_TICK: usize = 1_024;
const COLLISION_BLOCKS_GLOBAL_PER_TICK: usize = 2_048;
const CLIENT_PROTOCOL_VERSION: u8 = 1;

fn server_state() -> &'static Mutex<ServerState> {
    SKATE_STATE.get_or_init(|| Mutex::new(ServerState::default()))
}

fn supports_client_protocol(data: &[u8]) -> bool {
    data == [CLIENT_PROTOCOL_VERSION]
}

#[derive(Default)]
struct ServerState {
    compatible_clients: HashSet<String>,
    enabled: HashSet<String>,
    controls: HashMap<String, Controls>,
    control_updated_at: HashMap<String, Instant>,
    sessions: HashMap<String, SkateSession>,
    pending_loads: HashSet<String>,
    pending_initial_scans: HashMap<String, PendingSkateLoad>,
    engine_accumulator: HashMap<String, f32>,
    engine_updated_at: HashMap<String, Instant>,
    last_broadcast_at: HashMap<String, Instant>,
    collision_center: HashMap<String, (f32, f32, f32)>,
    collision_scans: HashMap<String, CollisionScan>,
    collision_scan_cursor: Option<String>,
}

struct PendingSkateLoad {
    scan: CollisionScan,
    spawn: [f32; 3],
    heading: f32,
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
            .register_event_handler::<ServerTickEndEvent, _>(
                SkateTick,
                EventPriority::Normal,
                false,
            )
            .map_err(|error| format!("could not register server tick handler: {error}"))?;
        context
            .register_event_handler::<pumpkin_plugin_api::events::player::PlayerLeaveEvent, _>(
                SkatePlayerLeave,
                EventPriority::Normal,
                false,
            )
            .map_err(|error| format!("could not register player cleanup handler: {error}"))?;
        context
            .register_event_handler::<PlayerDeathEvent, _>(
                SkatePlayerDeath,
                EventPriority::Lowest,
                false,
            )
            .map_err(|error| format!("could not register player death handler: {error}"))?;
        context
            .register_event_handler::<PlayerRespawnEvent, _>(
                SkatePlayerRespawn,
                EventPriority::Normal,
                false,
            )
            .map_err(|error| format!("could not register player respawn handler: {error}"))?;
        context
            .register_event_handler::<PlayerChangedWorldEvent, _>(
                SkatePlayerChangedWorld,
                EventPriority::Normal,
                false,
            )
            .map_err(|error| format!("could not register world change handler: {error}"))?;
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
        let Some(player) = server.get_player_by_name(&name) else {
            sender.send_message(TextComponent::text(
                "Skate mode can only be enabled by a player.",
            ));
            return Ok(0);
        };
        let should_disable = {
            let Ok(mut state) = server_state().lock() else {
                return Err(pumpkin_plugin_api::command::CommandError::CommandFailed(
                    TextComponent::text("Skate mode state is unavailable."),
                ));
            };
            if state.enabled.contains(&name) {
                clear_skate_state(&mut state, &name);
                true
            } else if !state.compatible_clients.contains(&name) {
                sender.send_message(TextComponent::text(
                    "Install the PEX Skate Fabric client and reconnect before enabling Skate mode.",
                ));
                return Ok(0);
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
            broadcast_state(
                &server,
                player.as_entity().get_id(),
                player.get_position(),
                player.get_yaw(),
                0.0,
                false,
            );
            sender.send_message(TextComponent::text("PEX Skate mode disabled."));
        } else {
            if DATA_ROOT.get().is_none() {
                clear_pending_load(&name);
                sender.send_message(TextComponent::text(
                    "Skate Engine data folder is unavailable.",
                ));
                return Ok(0);
            }
            let position = player.get_position();
            let spawn = [position.0 as f32, position.1 as f32, position.2 as f32];
            let heading = player.get_yaw().to_radians();
            let Ok(mut state) = server_state().lock() else {
                sender.send_message(TextComponent::text("Skate mode state is unavailable."));
                clear_pending_load(&name);
                return Ok(0);
            };
            state.pending_initial_scans.insert(
                name.clone(),
                PendingSkateLoad {
                    scan: CollisionScan::new(position),
                    spawn,
                    heading,
                },
            );
            sender.send_message(TextComponent::text(
                "Preparing Skate mode: sampling local Minecraft collision. This can take a moment.",
            ));
        }
        Ok(0)
    }
}

fn clear_skate_state(state: &mut ServerState, name: &str) -> bool {
    let had_skate_state = state.enabled.contains(name)
        || state.pending_loads.contains(name)
        || state.pending_initial_scans.contains_key(name)
        || state.sessions.contains_key(name);
    state.enabled.remove(name);
    state.controls.remove(name);
    state.control_updated_at.remove(name);
    state.sessions.remove(name);
    state.pending_loads.remove(name);
    state.pending_initial_scans.remove(name);
    state.engine_accumulator.remove(name);
    state.engine_updated_at.remove(name);
    state.last_broadcast_at.remove(name);
    state.collision_center.remove(name);
    state.collision_scans.remove(name);
    had_skate_state
}

fn clear_pending_load(name: &str) {
    if let Ok(mut state) = server_state().lock() {
        state.pending_loads.remove(name);
        state.pending_initial_scans.remove(name);
    }
}

struct SkateInput;

impl EventHandler<PlayerCustomPayloadEvent> for SkateInput {
    fn handle(
        &self,
        _server: Server,
        event: pumpkin_plugin_api::events::EventData<PlayerCustomPayloadEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerCustomPayloadEvent> {
        if event.channel == "pexskate:hello" {
            if supports_client_protocol(&event.data)
                && let Ok(mut state) = server_state().lock()
            {
                state.compatible_clients.insert(event.player.get_name());
            }
            return event;
        }
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
            state.compatible_clients.remove(&name);
            clear_skate_state(&mut state, &name);
        }
        broadcast_state(&server, entity_id, position, yaw, 0.0, false);
        event
    }
}

struct SkatePlayerDeath;

impl EventHandler<PlayerDeathEvent> for SkatePlayerDeath {
    fn handle(
        &self,
        server: Server,
        event: pumpkin_plugin_api::events::EventData<PlayerDeathEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerDeathEvent> {
        if !event.cancelled {
            stop_skate_for_lifecycle_event(&server, &event.player);
        }
        event
    }
}

struct SkatePlayerRespawn;

impl EventHandler<PlayerRespawnEvent> for SkatePlayerRespawn {
    fn handle(
        &self,
        server: Server,
        event: pumpkin_plugin_api::events::EventData<PlayerRespawnEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerRespawnEvent> {
        stop_skate_for_lifecycle_event(&server, &event.player);
        event
    }
}

struct SkatePlayerChangedWorld;

impl EventHandler<PlayerChangedWorldEvent> for SkatePlayerChangedWorld {
    fn handle(
        &self,
        server: Server,
        event: pumpkin_plugin_api::events::EventData<PlayerChangedWorldEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerChangedWorldEvent> {
        if !event.cancelled {
            stop_skate_for_lifecycle_event(&server, &event.player);
        }
        event
    }
}

fn stop_skate_for_lifecycle_event(server: &Server, player: &pumpkin_plugin_api::Player) {
    let name = player.get_name();
    let stopped = server_state()
        .lock()
        .map(|mut state| clear_skate_state(&mut state, &name))
        .unwrap_or(false);
    if stopped {
        broadcast_state(
            server,
            player.as_entity().get_id(),
            player.get_position(),
            player.get_yaw(),
            0.0,
            false,
        );
    }
}

struct SkateMovement;

impl EventHandler<PlayerMoveEvent> for SkateMovement {
    fn handle(
        &self,
        _server: Server,
        mut event: pumpkin_plugin_api::events::EventData<PlayerMoveEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerMoveEvent> {
        let name = event.player.get_name();
        let Ok(state) = server_state().lock() else {
            return event;
        };
        let Some(session) = state.sessions.get(&name) else {
            return event;
        };
        let pose = session.pose();
        event.to_position = (
            f64::from(pose.root[0]),
            f64::from(pose.root[1]),
            f64::from(pose.root[2]),
        );
        event
    }
}

struct SkateTick;

impl EventHandler<ServerTickEndEvent> for SkateTick {
    fn handle(
        &self,
        server: Server,
        event: pumpkin_plugin_api::events::EventData<ServerTickEndEvent>,
    ) -> pumpkin_plugin_api::events::EventData<ServerTickEndEvent> {
        let names = server_state()
            .lock()
            .map(|state| collision_work_order(&state))
            .unwrap_or_default();
        let mut collision_budget = COLLISION_BLOCKS_GLOBAL_PER_TICK;
        for name in names {
            let (is_pending, is_enabled) = server_state()
                .lock()
                .map(|state| {
                    (
                        state.pending_initial_scans.contains_key(&name),
                        state.enabled.contains(&name),
                    )
                })
                .unwrap_or_default();
            let remaining_before = collision_budget;
            if is_pending {
                tick_pending_skate_load(&server, &name, &mut collision_budget);
            } else if is_enabled {
                tick_skater(&server, &name, &mut collision_budget);
            }
            if collision_budget < remaining_before
                && let Ok(mut state) = server_state().lock()
            {
                state.collision_scan_cursor = Some(name);
            }
        }
        event
    }
}

fn collision_work_order(state: &ServerState) -> Vec<String> {
    let mut names = state
        .pending_initial_scans
        .keys()
        .chain(state.enabled.iter())
        .cloned()
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    if let Some(cursor) = state.collision_scan_cursor.as_deref()
        && let Some(next) = names.iter().position(|name| name.as_str() > cursor)
    {
        names.rotate_left(next);
    }
    names
}

fn take_collision_budget(global_budget: &mut usize) -> usize {
    let budget = (*global_budget).min(COLLISION_BLOCKS_PER_TICK);
    *global_budget -= budget;
    budget
}

fn tick_pending_skate_load(server: &Server, name: &str, global_budget: &mut usize) {
    let Some(player) = server.get_player_by_name(name) else {
        clear_pending_load(name);
        return;
    };
    let mut pending = server_state()
        .lock()
        .ok()
        .and_then(|mut state| state.pending_initial_scans.remove(name));
    let Some(mut pending) = pending.take() else {
        return;
    };
    let position = player.get_position();
    if initial_scan_recenter_needed(
        pending.scan.center(),
        (position.0 as f32, position.1 as f32, position.2 as f32),
    ) {
        pending.scan = CollisionScan::new(position);
        pending.spawn = [position.0 as f32, position.1 as f32, position.2 as f32];
        pending.heading = player.get_yaw().to_radians();
    }
    let world = player.as_entity().get_world();
    let block_budget = take_collision_budget(global_budget);
    if block_budget == 0 {
        if let Ok(mut state) = server_state().lock()
            && state.pending_loads.contains(name)
        {
            state.pending_initial_scans.insert(name.to_owned(), pending);
        }
        return;
    }
    let complete = match pending.scan.advance(&world, block_budget) {
        Ok(complete) => complete,
        Err(error) => {
            clear_pending_load(name);
            player.send_system_message(
                TextComponent::text(&format!("Could not read skate collision: {error}")),
                true,
            );
            return;
        }
    };
    if !complete {
        if let Ok(mut state) = server_state().lock() {
            if state.pending_loads.contains(name) {
                state.pending_initial_scans.insert(name.to_owned(), pending);
            }
        }
        return;
    }

    let collision_center = pending.scan.center();
    let Some(local) = pending
        .scan
        .into_local()
        .filter(|local| !local.triangles.is_empty())
    else {
        clear_pending_load(name);
        player.send_system_message(
            TextComponent::text("No Minecraft collision was found near you."),
            true,
        );
        return;
    };
    let Some(data_root) = DATA_ROOT.get() else {
        clear_pending_load(name);
        player.send_system_message(
            TextComponent::text("Skate Engine data folder is unavailable."),
            true,
        );
        return;
    };
    let spawn_position = player.get_position();
    pending.spawn = [
        spawn_position.0 as f32,
        spawn_position.1 as f32,
        spawn_position.2 as f32,
    ];
    pending.heading = player.get_yaw().to_radians();
    let session = match SkateSession::load(
        data_root,
        local.triangles,
        local.rails,
        pending.spawn,
        pending.heading,
    ) {
        Ok(session) => session,
        Err(error) => {
            clear_pending_load(name);
            player.send_system_message(
                TextComponent::text(&format!("Skate Engine could not load: {error}")),
                true,
            );
            return;
        }
    };
    let mut session = session;
    let mut pose = session.pose();
    pose.visual = session.skeletal_pose();
    let now = Instant::now();
    let Ok(mut state) = server_state().lock() else {
        clear_pending_load(name);
        player.send_system_message(
            TextComponent::text("Skate mode state is unavailable."),
            true,
        );
        return;
    };
    if !state.pending_loads.remove(name) {
        return;
    }
    state.pending_initial_scans.remove(name);
    state.enabled.insert(name.to_owned());
    state.controls.insert(name.to_owned(), Controls::default());
    state.control_updated_at.insert(name.to_owned(), now);
    state.sessions.insert(name.to_owned(), session);
    state.engine_accumulator.insert(name.to_owned(), 0.0);
    state.engine_updated_at.insert(name.to_owned(), now);
    state.last_broadcast_at.insert(name.to_owned(), now);
    state
        .collision_center
        .insert(name.to_owned(), collision_center);
    drop(state);
    broadcast_pose(server, player.as_entity().get_id(), &pose, true);
    player.send_system_message(TextComponent::text("Skate 3 Engine mode enabled."), true);
}

fn tick_skater(server: &Server, name: &str, global_collision_budget: &mut usize) {
    let Some(player) = server.get_player_by_name(name) else {
        return;
    };
    let now = Instant::now();
    let (pose, should_refresh_collision, should_broadcast) = {
        let Ok(mut state) = server_state().lock() else {
            return;
        };
        if !state.enabled.contains(name) {
            return;
        }
        let controls = state.controls_for(name, now);
        let previous = state
            .engine_updated_at
            .insert(name.to_owned(), now)
            .unwrap_or(now);
        let accumulated = state.engine_accumulator.entry(name.to_owned()).or_default();
        *accumulated += now
            .saturating_duration_since(previous)
            .as_secs_f32()
            .min(0.25);
        let mut remaining = *accumulated;
        let Some(session) = state.sessions.get_mut(name) else {
            return;
        };
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
        if let Some(error) = failure {
            clear_skate_state(&mut state, name);
            drop(state);
            let entity_id = player.as_entity().get_id();
            let position = player.get_position();
            broadcast_state(server, entity_id, position, player.get_yaw(), 0.0, false);
            player.send_system_message(
                TextComponent::text(&format!(
                    "Skate Engine stopped after a simulation error: {error}"
                )),
                true,
            );
            return;
        }
        state.engine_accumulator.insert(name.to_owned(), remaining);
        let center = state.collision_center.get(name).copied().unwrap_or((
            pose.root[0],
            pose.root[1],
            pose.root[2],
        ));
        let should_refresh_collision = collision_refresh_needed(center, pose.root);
        let should_broadcast = state
            .last_broadcast_at
            .get(name)
            .is_none_or(|last| now.saturating_duration_since(*last) >= STATE_BROADCAST_INTERVAL);
        if should_broadcast {
            state.last_broadcast_at.insert(name.to_owned(), now);
            if let Some(session) = state.sessions.get_mut(name) {
                pose.visual = session.skeletal_pose();
            }
        }
        (pose, should_refresh_collision, should_broadcast)
    };

    let root = (
        f64::from(pose.root[0]),
        f64::from(pose.root[1]),
        f64::from(pose.root[2]),
    );
    let world = player.as_entity().get_world();
    if let Err(error) = advance_collision_scan(
        &world,
        name,
        should_refresh_collision,
        root,
        global_collision_budget,
    ) {
        stop_skate_for_player(server, &player, name, error);
        return;
    }

    let current = player.get_position();
    let dx = current.0 - root.0;
    let dy = current.1 - root.1;
    let dz = current.2 - root.2;
    if dx * dx + dy * dy + dz * dz > 0.0001 {
        player.teleport(
            root,
            Some(player.get_yaw()),
            Some(player.get_pitch()),
            world,
        );
    }
    if should_broadcast {
        broadcast_pose(server, player.as_entity().get_id(), &pose, true);
    }
}

fn advance_collision_scan(
    world: &pumpkin_plugin_api::World,
    name: &str,
    start_if_needed: bool,
    requested_center: (f64, f64, f64),
    global_budget: &mut usize,
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
    let block_budget = take_collision_budget(global_budget);
    if block_budget == 0 {
        server_state()
            .lock()
            .map_err(|_| "Skate mode state is unavailable".to_owned())?
            .collision_scans
            .insert(name.to_owned(), scan);
        return Ok(());
    }
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

fn stop_skate_for_player(
    server: &Server,
    player: &pumpkin_plugin_api::Player,
    name: &str,
    error: String,
) {
    if let Ok(mut state) = server_state().lock() {
        clear_skate_state(&mut state, name);
    }
    let position = player.get_position();
    player.send_system_message(
        TextComponent::text(&format!(
            "Skate Engine stopped while updating local collision: {error}"
        )),
        true,
    );
    broadcast_state(
        server,
        player.as_entity().get_id(),
        position,
        player.get_yaw(),
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

fn initial_scan_recenter_needed(center: (f32, f32, f32), position: (f32, f32, f32)) -> bool {
    let dx = position.0 - center.0;
    let dy = position.1 - center.1;
    let dz = position.2 - center.2;
    dx * dx + dz * dz >= 16.0 || dy.abs() >= 1.5
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
        CLIENT_PROTOCOL_VERSION, COLLISION_BLOCKS_GLOBAL_PER_TICK, COLLISION_BLOCKS_PER_TICK,
        COLLISION_REFRESH_VERTICAL_DISTANCE, INPUT_TIMEOUT, PendingSkateLoad, ServerState,
        append_visual, clear_skate_state, collision::CollisionScan, collision_refresh_needed,
        collision_work_order, engine, initial_scan_recenter_needed, physics::Controls,
        supports_client_protocol, take_collision_budget,
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
    fn initial_collision_scan_recenters_if_player_moves_out_of_its_safe_window() {
        let center = (0.0, 64.0, 0.0);
        assert!(!initial_scan_recenter_needed(center, (3.99, 64.0, 0.0)));
        assert!(initial_scan_recenter_needed(center, (4.0, 64.0, 0.0)));
        assert!(initial_scan_recenter_needed(center, (0.0, 65.5, 0.0)));
    }

    #[test]
    fn collision_scan_order_rotates_after_last_player_that_used_budget() {
        let mut state = ServerState::default();
        state
            .enabled
            .extend(["alpha".into(), "bravo".into(), "charlie".into()]);
        state.collision_scan_cursor = Some("alpha".into());
        assert_eq!(
            collision_work_order(&state),
            vec!["bravo", "charlie", "alpha"]
        );
        state.collision_scan_cursor = Some("charlie".into());
        assert_eq!(
            collision_work_order(&state),
            vec!["alpha", "bravo", "charlie"]
        );
    }

    #[test]
    fn per_player_collision_work_never_exceeds_the_global_tick_budget() {
        let mut remaining = COLLISION_BLOCKS_GLOBAL_PER_TICK;
        assert_eq!(
            take_collision_budget(&mut remaining),
            COLLISION_BLOCKS_PER_TICK
        );
        assert_eq!(
            take_collision_budget(&mut remaining),
            COLLISION_BLOCKS_PER_TICK
        );
        assert_eq!(take_collision_budget(&mut remaining), 0);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn handshake_requires_the_supported_client_protocol_version() {
        assert!(supports_client_protocol(&[CLIENT_PROTOCOL_VERSION]));
        assert!(!supports_client_protocol(&[2]));
        assert!(!supports_client_protocol(&[CLIENT_PROTOCOL_VERSION, 0]));
        assert!(!supports_client_protocol(&[]));
    }

    #[test]
    fn lifecycle_cleanup_removes_active_and_pending_player_state() {
        let mut state = ServerState::default();
        state.compatible_clients.insert("rider".into());
        state.enabled.insert("rider".into());
        state.pending_loads.insert("rider".into());
        state.pending_initial_scans.insert(
            "rider".into(),
            PendingSkateLoad {
                scan: CollisionScan::new((0.0, 64.0, 0.0)),
                spawn: [0.0, 64.0, 0.0],
                heading: 0.0,
            },
        );
        state.controls.insert("rider".into(), Controls::default());
        state
            .control_updated_at
            .insert("rider".into(), Instant::now());
        state.engine_accumulator.insert("rider".into(), 0.05);
        state
            .engine_updated_at
            .insert("rider".into(), Instant::now());
        state
            .last_broadcast_at
            .insert("rider".into(), Instant::now());
        state
            .collision_center
            .insert("rider".into(), (0.0, 64.0, 0.0));
        state
            .collision_scans
            .insert("rider".into(), CollisionScan::new((0.0, 64.0, 0.0)));

        assert!(clear_skate_state(&mut state, "rider"));
        assert!(!clear_skate_state(&mut state, "rider"));
        assert!(state.compatible_clients.contains("rider"));
        assert!(!state.enabled.contains("rider"));
        assert!(!state.pending_loads.contains("rider"));
        assert!(!state.pending_initial_scans.contains_key("rider"));
        assert!(!state.controls.contains_key("rider"));
        assert!(!state.control_updated_at.contains_key("rider"));
        assert!(!state.engine_accumulator.contains_key("rider"));
        assert!(!state.engine_updated_at.contains_key("rider"));
        assert!(!state.last_broadcast_at.contains_key("rider"));
        assert!(!state.collision_center.contains_key("rider"));
        assert!(!state.collision_scans.contains_key("rider"));
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
