mod collision;
mod engine;
mod physics;
mod prop_physics;

use collision::CollisionScan;
use engine::{SkatePose, SkateSession};
use physics::{Controls, decode_sequenced};
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
    permission::{Permission, PermissionDefault},
    text::TextComponent,
};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
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
const CLIENT_PROTOCOL_VERSION: u8 = 8;
const PLAYER_IMPACT_COOLDOWN: Duration = Duration::from_millis(1_250);

fn server_state() -> &'static Mutex<ServerState> {
    SKATE_STATE.get_or_init(|| Mutex::new(ServerState::default()))
}

fn supports_client_protocol(data: &[u8]) -> bool {
    data == [CLIENT_PROTOCOL_VERSION]
}

fn sequence_is_newer(candidate: u32, previous: u32) -> bool {
    (candidate.wrapping_sub(previous) as i32) > 0
}

#[derive(Default)]
struct ServerState {
    compatible_clients: HashSet<String>,
    enabled: HashSet<String>,
    controls: HashMap<String, Controls>,
    control_updated_at: HashMap<String, Instant>,
    control_sequences: HashMap<String, u32>,
    acknowledged_control_sequences: HashMap<String, u32>,
    sessions: HashMap<String, SkateSession>,
    map_backed: HashSet<String>,
    map_filenames: HashMap<String, String>,
    map_digests: HashMap<String, MapDigests>,
    prop_world_keys: HashMap<String, PropWorldKey>,
    prop_worlds: HashMap<PropWorldKey, prop_physics::PropWorld>,
    prop_world_advanced_at: HashMap<PropWorldKey, Instant>,
    prop_sent_revisions: HashMap<String, (PropWorldKey, u64)>,
    pending_loads: HashSet<String>,
    pending_initial_scans: HashMap<String, PendingSkateLoad>,
    engine_accumulator: HashMap<String, f32>,
    engine_updated_at: HashMap<String, Instant>,
    last_broadcast_at: HashMap<String, Instant>,
    collision_center: HashMap<String, (f32, f32, f32)>,
    collision_scans: HashMap<String, CollisionScan>,
    collision_scan_cursor: Option<String>,
    collision_poses: HashMap<String, SkatePose>,
    collision_impacts: HashMap<(String, String), Instant>,
}

#[derive(Clone, Debug)]
struct MapDigests {
    map: String,
    props: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PropWorldKey {
    world_id: String,
    map_filename: String,
    props_digest: String,
}

impl PropWorldKey {
    fn new(world_id: String, map_filename: &str, props_digest: &str) -> Self {
        Self {
            world_id,
            map_filename: map_filename.to_ascii_lowercase(),
            props_digest: props_digest.to_ascii_lowercase(),
        }
    }
}

struct PendingSkateLoad {
    scan: CollisionScan,
    spawn: [f32; 3],
    heading: f32,
}

impl ServerState {
    fn controls_are_fresh(&self, name: &str, now: Instant) -> bool {
        self.control_updated_at
            .get(name)
            .is_some_and(|updated| now.saturating_duration_since(*updated) <= INPUT_TIMEOUT)
    }

    fn controls_for(&self, name: &str, now: Instant) -> Controls {
        if self.controls_are_fresh(name, now) {
            self.controls.get(name).copied().unwrap_or_default()
        } else {
            Controls::default()
        }
    }

    fn acknowledge_control_step(&mut self, name: &str, steps: usize, now: Instant) -> u32 {
        if steps > 0
            && self.controls_are_fresh(name, now)
            && let Some(sequence) = self.control_sequences.get(name).copied()
        {
            self.acknowledged_control_sequences
                .insert(name.to_owned(), sequence);
        }
        self.acknowledged_control_sequences
            .get(name)
            .copied()
            .unwrap_or(0)
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
        context
            .register_permission(&Permission {
                node: "pex-skate:use".into(),
                description: "Allows players to enter or leave PEX Skate mode.".into(),
                default: PermissionDefault::Allow,
                children: Default::default(),
            })
            .map_err(|error| format!("could not register the Skate gameplay permission: {error}"))?;
        let command = Command::new(&["skate".into()], "Toggle PEX Skate mode").execute(ToggleSkate);
        context.register_command(command, "pex-skate:use");

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
            let Some(data_root) = DATA_ROOT.get() else {
                clear_pending_load(&name);
                sender.send_message(TextComponent::text(
                    "Skate Engine data folder is unavailable.",
                ));
                return Ok(0);
            };
            match default_skate_map_path(data_root) {
                Ok(Some(map_path)) => {
                    let map_digests = match load_map_digests(data_root, &map_path) {
                        Ok(digests) => digests,
                        Err(error) => {
                            clear_pending_load(&name);
                            sender.send_message(TextComponent::text(&format!(
                                "Could not verify Skate map {}: {error}",
                                map_path.file_name().unwrap_or_default().to_string_lossy()
                            )));
                            return Ok(0);
                        }
                    };
                    let session = match SkateSession::load_skate_map(data_root, &map_path) {
                        Ok(session) => session,
                        Err(error) => {
                            clear_pending_load(&name);
                            sender.send_message(TextComponent::text(&format!(
                                "Could not load Skate map {}: {error}",
                                map_path.file_name().unwrap_or_default().to_string_lossy()
                            )));
                            return Ok(0);
                        }
                    };
                    let mut session = session;
                    let mut pose = session.pose();
                    pose.visual = session.skeletal_pose();
                    let root = (
                        f64::from(pose.root[0]),
                        f64::from(pose.root[1]),
                        f64::from(pose.root[2]),
                    );
                    let map_filename = map_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let prop_world_key = PropWorldKey::new(
                        player.as_entity().get_world().get_id().to_string(),
                        &map_filename,
                        &map_digests.props,
                    );
                    let now = Instant::now();
                    let Ok(mut state) = server_state().lock() else {
                        clear_pending_load(&name);
                        sender
                            .send_message(TextComponent::text("Skate mode state is unavailable."));
                        return Ok(0);
                    };
                    if !state.pending_loads.remove(&name) {
                        return Ok(0);
                    }
                    let initial_prop_world =
                        prop_physics::PropWorld::new(session.prop_physics_bodies());
                    let shared_prop_transforms = state
                        .prop_worlds
                        .entry(prop_world_key.clone())
                        .or_insert(initial_prop_world)
                        .transforms();
                    if let Err(error) = session.update_prop_transforms(&shared_prop_transforms) {
                        state.pending_loads.remove(&name);
                        if !state
                            .prop_world_keys
                            .values()
                            .any(|key| key == &prop_world_key)
                        {
                            state.prop_worlds.remove(&prop_world_key);
                            state.prop_world_advanced_at.remove(&prop_world_key);
                        }
                        drop(state);
                        sender.send_message(TextComponent::text(&format!(
                            "Could not initialize shared Skate prop collision: {error}"
                        )));
                        return Ok(0);
                    }
                    state.pending_initial_scans.remove(&name);
                    state.enabled.insert(name.clone());
                    state.map_backed.insert(name.clone());
                    state
                        .map_filenames
                        .insert(name.clone(), map_filename.clone());
                    state.map_digests.insert(name.clone(), map_digests.clone());
                    state
                        .prop_world_keys
                        .insert(name.clone(), prop_world_key.clone());
                    state
                        .prop_sent_revisions
                        .insert(name.clone(), (prop_world_key.clone(), 0));
                    state
                        .prop_world_advanced_at
                        .entry(prop_world_key.clone())
                        .or_insert(now);
                    state.controls.insert(name.clone(), Controls::default());
                    state.control_updated_at.insert(name.clone(), now);
                    state.sessions.insert(name.clone(), session);
                    state.collision_poses.insert(name.clone(), pose.clone());
                    state.engine_accumulator.insert(name.clone(), 0.0);
                    state.engine_updated_at.insert(name.clone(), now);
                    state.last_broadcast_at.insert(name.clone(), now);
                    state
                        .collision_center
                        .insert(name.clone(), (root.0 as f32, root.1 as f32, root.2 as f32));
                    let (prop_updates, prop_revision) = state
                        .prop_worlds
                        .get(&prop_world_key)
                        .map(|world| world.changes_since(0))
                        .unwrap_or_default();
                    state
                        .prop_sent_revisions
                        .insert(name.clone(), (prop_world_key, prop_revision));
                    drop(state);
                    let world = player.as_entity().get_world();
                    player.teleport(
                        root,
                        Some(player.get_yaw()),
                        Some(player.get_pitch()),
                        world,
                    );
                    broadcast_pose(
                        &server,
                        player.as_entity().get_id(),
                        &pose,
                        true,
                        true,
                        Some(&map_filename),
                        Some(&map_digests),
                        0,
                        &prop_updates,
                    );
                    sender.send_message(TextComponent::text(&format!(
                        "Skate 3 Engine mode enabled on {}.",
                        map_path.file_stem().unwrap_or_default().to_string_lossy()
                    )));
                    return Ok(0);
                }
                Ok(None) => {}
                Err(error) => {
                    clear_pending_load(&name);
                    sender.send_message(TextComponent::text(&format!(
                        "Could not select an installed Skate map: {error}"
                    )));
                    return Ok(0);
                }
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
        || state.sessions.contains_key(name)
        || state.map_backed.contains(name);
    state.enabled.remove(name);
    state.controls.remove(name);
    state.control_updated_at.remove(name);
    state.control_sequences.remove(name);
    state.acknowledged_control_sequences.remove(name);
    state.sessions.remove(name);
    state.map_backed.remove(name);
    state.map_filenames.remove(name);
    state.map_digests.remove(name);
    if let Some(prop_world_key) = state.prop_world_keys.remove(name)
        && !state
            .prop_world_keys
            .values()
            .any(|key| key == &prop_world_key)
    {
        state.prop_worlds.remove(&prop_world_key);
        state.prop_world_advanced_at.remove(&prop_world_key);
    }
    state.prop_sent_revisions.remove(name);
    state.pending_loads.remove(name);
    state.pending_initial_scans.remove(name);
    state.engine_accumulator.remove(name);
    state.engine_updated_at.remove(name);
    state.last_broadcast_at.remove(name);
    state.collision_center.remove(name);
    state.collision_scans.remove(name);
    state.collision_poses.remove(name);
    state
        .collision_impacts
        .retain(|(left, right), _| left != name && right != name);
    had_skate_state
}

fn clear_pending_load(name: &str) {
    if let Ok(mut state) = server_state().lock() {
        state.pending_loads.remove(name);
        state.pending_initial_scans.remove(name);
    }
}

fn load_map_digests(data_root: &Path, map_path: &Path) -> Result<MapDigests, String> {
    let map_bytes = std::fs::read(map_path)
        .map_err(|error| format!("could not read {}: {error}", map_path.display()))?;
    let map = skate_data::sha256_digest(&map_bytes);
    let stem = map_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .ok_or_else(|| "Skate map filename is invalid".to_owned())?;
    let props_path = data_root
        .join("private")
        .join("native-props")
        .join(format!("{stem}.skate"));
    let props = match std::fs::read(&props_path) {
        Ok(bytes) => skate_data::sha256_digest(&bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("could not read {}: {error}", props_path.display())),
    };
    Ok(MapDigests { map, props })
}

/// Select a converted map deterministically. An optional settings/default-map.txt
/// may name a map filename; otherwise prefer the main University district and
/// then the first package in case-insensitive filename order.
fn default_skate_map_path(data_root: &Path) -> Result<Option<PathBuf>, String> {
    let maps_root = data_root.join("maps");
    let entries = match std::fs::read_dir(&maps_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", maps_root.display())),
    };
    let mut maps = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not inspect installed maps: {error}"))?;
        let kind = entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?;
        if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("skate"))
        {
            maps.push(entry.path());
        }
    }
    maps.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    });
    if maps.is_empty() {
        return Ok(None);
    }

    let selection_path = data_root.join("settings").join("default-map.txt");
    match std::fs::read_to_string(&selection_path) {
        Ok(selection) => {
            let selection = selection.trim();
            if !selection.is_empty() {
                let filename = Path::new(selection).file_name();
                if Path::new(selection).components().count() != 1 || filename.is_none() {
                    return Err(format!(
                        "{} must contain a single map filename",
                        selection_path.display()
                    ));
                }
                let selected = maps.iter().find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(selection))
                });
                return selected
                    .cloned()
                    .map(Some)
                    .ok_or_else(|| format!("{} is not installed in maps/", selection));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not read {}: {error}",
                selection_path.display()
            ));
        }
    }
    Ok(maps
        .iter()
        .find(|path| {
            path.file_stem()
                .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("University"))
        })
        .cloned()
        .or_else(|| maps.into_iter().next()))
}

struct SkateInput;

impl EventHandler<PlayerCustomPayloadEvent> for SkateInput {
    fn handle(
        &self,
        server: Server,
        event: pumpkin_plugin_api::events::EventData<PlayerCustomPayloadEvent>,
    ) -> pumpkin_plugin_api::events::EventData<PlayerCustomPayloadEvent> {
        if event.channel == "pexskate:hello" {
            if supports_client_protocol(&event.data) {
                let accepted = server_state()
                    .lock()
                    .map(|mut state| {
                        state.compatible_clients.insert(event.player.get_name());
                    })
                    .is_ok();
                if accepted && let Some(java_player) = event.player.as_java() {
                    java_player
                        .send_custom_payload("pexskate:hello_ack", &[CLIENT_PROTOCOL_VERSION]);
                }
            }
            return event;
        }
        if event.channel != "pexskate:control" {
            return event;
        }
        let Some((controls, sequence)) = decode_sequenced(&event.data) else {
            return event;
        };
        let name = event.player.get_name();
        let now = Instant::now();
        let world_id = event.player.as_entity().get_world().get_id();
        let shove_pose = if let Ok(mut state) = server_state().lock()
            && state.enabled.contains(&name)
        {
            if state
                .control_sequences
                .get(&name)
                .is_some_and(|previous| !sequence_is_newer(sequence, *previous))
            {
                return event;
            }
            state.control_sequences.insert(name.clone(), sequence);
            let previous_is_fresh = state
                .control_updated_at
                .get(&name)
                .is_some_and(|updated| now.saturating_duration_since(*updated) <= INPUT_TIMEOUT);
            let previous = if previous_is_fresh {
                state.controls.get(&name).copied().unwrap_or_default()
            } else {
                Controls::default()
            };
            state.controls.insert(name.clone(), controls);
            state.control_updated_at.insert(name.clone(), now);
            if shove_pressed_edge(controls, previous, previous_is_fresh) {
                state
                    .sessions
                    .get_mut(&name)
                    .and_then(|session| session.start_shove().then(|| session.pose()))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(pose) = shove_pose {
            apply_shove_to_nearest_skater(&server, &name, &world_id, &pose);
        }
        event
    }
}

fn apply_shove_to_nearest_skater(
    server: &Server,
    attacker: &str,
    world_id: &str,
    pose: &SkatePose,
) {
    let candidates = server_state()
        .lock()
        .map(|state| {
            state
                .sessions
                .iter()
                .filter(|(name, _)| name.as_str() != attacker)
                .map(|(name, session)| (name.clone(), session.pose()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let nearest = candidates
        .into_iter()
        .filter_map(|(name, target_pose)| {
            let player = server.get_player_by_name(&name)?;
            if player.as_entity().get_world().get_id() != world_id {
                return None;
            }
            let (velocity, distance_squared) = shove_impact(pose, target_pose.root)?;
            Some((name, velocity, distance_squared))
        })
        .min_by(|left, right| left.2.total_cmp(&right.2));

    if let Some((name, velocity, _)) = nearest
        && let Ok(mut state) = server_state().lock()
        && let Some(target) = state.sessions.get_mut(&name)
    {
        target.knock_down(velocity);
    }
}

fn shove_pressed_edge(current: Controls, previous: Controls, previous_is_fresh: bool) -> bool {
    current.shove && (!previous_is_fresh || !previous.shove)
}

fn shove_impact(attacker: &SkatePose, target: [f32; 3]) -> Option<([f32; 3], f32)> {
    let [qx, qy, qz, qw] = [
        attacker.board[3],
        attacker.board[4],
        attacker.board[5],
        attacker.board[6],
    ];
    let forward = [
        2.0 * (qx * qz + qw * qy),
        2.0 * (qy * qz - qw * qx),
        1.0 - 2.0 * (qx * qx + qy * qy),
    ];
    let forward_length = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
    if !forward_length.is_finite() || forward_length < 1.0e-5 {
        return None;
    }
    let forward = [forward[0] / forward_length, forward[2] / forward_length];
    let delta = [target[0] - attacker.root[0], target[2] - attacker.root[2]];
    let distance_squared = delta[0] * delta[0] + delta[1] * delta[1];
    const MAX_RANGE_SQUARED: f32 = 2.25;
    if !distance_squared.is_finite()
        || distance_squared < 0.01
        || distance_squared > MAX_RANGE_SQUARED
        || (target[1] - attacker.root[1]).abs() > 1.25
    {
        return None;
    }
    let distance = distance_squared.sqrt();
    let direction = [delta[0] / distance, delta[1] / distance];
    if direction[0] * forward[0] + direction[1] * forward[1] < 0.35 {
        return None;
    }
    Some(([forward[0] * 3.0, 1.25, forward[1] * 3.0], distance_squared))
}

fn player_collision_impact(attacker: &SkatePose, target: &SkatePose) -> Option<[f32; 3]> {
    let board = &attacker.board;
    let target_root = target.root;
    let values = attacker
        .velocity
        .iter()
        .chain(target.velocity.iter())
        .chain(board.iter())
        .chain(target_root.iter());
    if values.clone().any(|value| !value.is_finite()) {
        return None;
    }
    let dx = target_root[0] - board[0];
    let dz = target_root[2] - board[2];
    let horizontal_distance_squared = dx * dx + dz * dz;
    if horizontal_distance_squared > 0.36
        || horizontal_distance_squared < 1e-4
        || board[1] < target_root[1] - 0.25
        || board[1] > target_root[1] + 1.55
    {
        return None;
    }

    let relative_x = attacker.velocity[0] - target.velocity[0];
    let relative_z = attacker.velocity[2] - target.velocity[2];
    let relative_speed = (relative_x * relative_x + relative_z * relative_z).sqrt();
    if relative_speed < 5.5 {
        return None;
    }

    let direction_length = horizontal_distance_squared.sqrt();
    let direction = [dx / direction_length, dz / direction_length];
    let magnitude = (2.5 + relative_speed * 0.35).min(8.0);
    Some([
        direction[0] * magnitude,
        (1.8 + relative_speed * 0.08).min(3.0),
        direction[1] * magnitude,
    ])
}

/// Detect a board-to-rider hit along authoritative poses sampled on the
/// previous and current server updates. This closes the tunnelling gap in the
/// point-only check when a fast skater travels through a target between
/// broadcasts.
fn swept_player_collision_impact(
    attacker_previous: &SkatePose,
    attacker: &SkatePose,
    target_previous: &SkatePose,
    target: &SkatePose,
) -> Option<[f32; 3]> {
    let values = attacker_previous
        .board
        .iter()
        .chain(attacker.board.iter())
        .chain(attacker.velocity.iter())
        .chain(target_previous.root.iter())
        .chain(target.root.iter())
        .chain(target.velocity.iter());
    if values.clone().any(|value| !value.is_finite()) {
        return None;
    }

    let relative_start = [
        target_previous.root[0] - attacker_previous.board[0],
        target_previous.root[1] - attacker_previous.board[1],
        target_previous.root[2] - attacker_previous.board[2],
    ];
    let relative_end = [
        target.root[0] - attacker.board[0],
        target.root[1] - attacker.board[1],
        target.root[2] - attacker.board[2],
    ];
    let relative_delta =
        std::array::from_fn::<_, 3, _>(|axis| relative_end[axis] - relative_start[axis]);
    // The rider proxy is a vertical column. Find closest horizontal approach
    // first, then check the vertical span at that same point in time.
    let denominator = relative_delta[0] * relative_delta[0] + relative_delta[2] * relative_delta[2];
    let closest_time = if denominator > 1.0e-8 {
        (-(relative_start[0] * relative_delta[0] + relative_start[2] * relative_delta[2])
            / denominator)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let contact = std::array::from_fn::<_, 3, _>(|axis| {
        relative_start[axis] + relative_delta[axis] * closest_time
    });
    let horizontal_distance_squared = contact[0] * contact[0] + contact[2] * contact[2];
    if horizontal_distance_squared > 0.36 || contact[1] < -0.25 || contact[1] > 1.55 {
        return None;
    }

    let relative_velocity = [
        attacker.velocity[0] - target.velocity[0],
        attacker.velocity[2] - target.velocity[2],
    ];
    let relative_speed = (relative_velocity[0] * relative_velocity[0]
        + relative_velocity[1] * relative_velocity[1])
        .sqrt();
    if !relative_speed.is_finite() || relative_speed < 5.5 {
        return None;
    }

    let direction = if horizontal_distance_squared > 1.0e-4 {
        let length = horizontal_distance_squared.sqrt();
        [contact[0] / length, contact[2] / length]
    } else {
        [
            relative_velocity[0] / relative_speed,
            relative_velocity[1] / relative_speed,
        ]
    };
    let magnitude = (2.5 + relative_speed * 0.35).min(8.0);
    Some([
        direction[0] * magnitude,
        (1.8 + relative_speed * 0.08).min(3.0),
        direction[1] * magnitude,
    ])
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
    state.collision_poses.insert(name.to_owned(), pose.clone());
    state.engine_accumulator.insert(name.to_owned(), 0.0);
    state.engine_updated_at.insert(name.to_owned(), now);
    state.last_broadcast_at.insert(name.to_owned(), now);
    state
        .collision_center
        .insert(name.to_owned(), collision_center);
    drop(state);
    broadcast_pose(
        server,
        player.as_entity().get_id(),
        &pose,
        true,
        false,
        None,
        None,
        0,
        &[],
    );
    player.send_system_message(TextComponent::text("Skate 3 Engine mode enabled."), true);
}

fn tick_skater(server: &Server, name: &str, global_collision_budget: &mut usize) {
    let Some(player) = server.get_player_by_name(name) else {
        return;
    };
    let now = Instant::now();
    let world_id = player.as_entity().get_world().get_id();
    let player_position = player.get_position();
    let (
        pose,
        should_refresh_collision,
        should_broadcast,
        map_backed,
        map_filename,
        map_digests,
        acknowledged_sequence,
        prop_updates,
    ) =
        {
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
            let current_map_backed = state.map_backed.contains(name);
            let current_map = state.map_filenames.get(name);
            // Client movement events and engine ticks can arrive at different
            // cadences. Use the authoritative Skate pose for broad-phase
            // proximity instead of the last Minecraft movement packet.
            let query_center = state
                .sessions
                .get(name)
                .map(|session| session.pose().root)
                .unwrap_or([
                    player_position.0 as f32,
                    player_position.1 as f32,
                    player_position.2 as f32,
                ]);
            let mut nearby_players = Vec::new();
            for (other_name, other_session) in &state.sessions {
                if other_name == name
                    || state.map_backed.contains(other_name) != current_map_backed
                    || (current_map_backed && state.map_filenames.get(other_name) != current_map)
                {
                    continue;
                }
                let other_pose = other_session.pose();
                let dx = other_pose.root[0] - query_center[0];
                let dy = other_pose.root[1] - query_center[1];
                let dz = other_pose.root[2] - query_center[2];
                // Keep the path length of a fast board inside the query even
                // when the rider has crossed past this player since the last
                // server update. The candidate list remains bounded below.
                if dx * dx + dy * dy + dz * dz > 36.0 {
                    continue;
                }
                let Some(other_player) = server.get_player_by_name(other_name) else {
                    continue;
                };
                if other_player.as_entity().get_world().get_id() != world_id {
                    continue;
                }
                nearby_players.push((other_name.clone(), other_pose, dx * dx + dy * dy + dz * dz));
            }
            nearby_players.sort_by(|left, right| {
                left.2
                    .total_cmp(&right.2)
                    .then_with(|| left.0.cmp(&right.0))
            });
            nearby_players.truncate(16);
            let previous_pose = state.collision_poses.get(name).cloned();
            let previous_nearby_poses = nearby_players
                .iter()
                .map(|(other_name, other_pose, _)| {
                    (
                        other_name.clone(),
                        state
                            .collision_poses
                            .get(other_name)
                            .cloned()
                            .unwrap_or_else(|| other_pose.clone()),
                    )
                })
                .collect::<HashMap<_, _>>();
            let prop_world_key = state.prop_world_keys.get(name).cloned();
            let shared_prop_transforms = prop_world_key
                .as_ref()
                .and_then(|key| state.prop_worlds.get(key))
                .map(prop_physics::PropWorld::transforms)
                .unwrap_or_default();
            let Some(session) = state.sessions.get_mut(name) else {
                return;
            };
            let period = session.period().clamp(1.0 / 120.0, 1.0 / 20.0);
            let mut pose = session.pose();
            let mut steps = 0;
            let collision_players: Vec<_> = nearby_players
                .iter()
                .map(|(_, player_pose, _)| player_pose.clone())
                .collect();
            let mut failure = session
                .update_prop_transforms(shared_prop_transforms.as_slice())
                .err()
                .or_else(|| session.refresh_player_collision(&collision_players).err());
            while remaining >= period && steps < 6 {
                if failure.is_some() {
                    break;
                }
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
            let previous_pose = previous_pose.unwrap_or_else(|| pose.clone());
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
            if let Some(key) = prop_world_key.as_ref() {
                let previous_step = state
                    .prop_world_advanced_at
                    .insert(key.clone(), now)
                    .unwrap_or(now);
                let delta = now.saturating_duration_since(previous_step).as_secs_f32();
                if let Some(world) = state.prop_worlds.get_mut(&key) {
                    world.step(
                        [
                            previous_pose.board[0],
                            previous_pose.board[1],
                            previous_pose.board[2],
                        ],
                        [pose.board[0], pose.board[1], pose.board[2]],
                        pose.velocity,
                        delta,
                    );
                }
            }
            let acknowledged_sequence = state.acknowledge_control_step(name, steps, now);
            for (other_name, other_pose, _) in &nearby_players {
                let previous_other_pose =
                    previous_nearby_poses.get(other_name).unwrap_or(other_pose);
                let Some(velocity) = swept_player_collision_impact(
                    &previous_pose,
                    &pose,
                    previous_other_pose,
                    other_pose,
                )
                .or_else(|| player_collision_impact(&pose, other_pose)) else {
                    continue;
                };
                let pair = if name < other_name.as_str() {
                    (name.to_owned(), other_name.clone())
                } else {
                    (other_name.clone(), name.to_owned())
                };
                if state.collision_impacts.get(&pair).is_some_and(|last| {
                    now.saturating_duration_since(*last) < PLAYER_IMPACT_COOLDOWN
                }) {
                    continue;
                }
                let knocked_pose = state
                    .sessions
                    .get_mut(other_name)
                    .and_then(|target| target.knock_down(velocity).then(|| target.pose()));
                if let Some(knocked_pose) = knocked_pose {
                    state.collision_impacts.insert(pair, now);
                    state
                        .collision_poses
                        .insert(other_name.clone(), knocked_pose);
                }
            }
            state.collision_poses.insert(name.to_owned(), pose.clone());
            state.engine_accumulator.insert(name.to_owned(), remaining);
            let center = state.collision_center.get(name).copied().unwrap_or((
                pose.root[0],
                pose.root[1],
                pose.root[2],
            ));
            let map_backed = state.map_backed.contains(name);
            let map_filename = state.map_filenames.get(name).cloned();
            let map_digests = state.map_digests.get(name).cloned();
            let should_refresh_collision = should_refresh_collision(map_backed, center, pose.root);
            let should_broadcast = state.last_broadcast_at.get(name).is_none_or(|last| {
                now.saturating_duration_since(*last) >= STATE_BROADCAST_INTERVAL
            });
            let mut prop_updates = Vec::new();
            if should_broadcast {
                state.last_broadcast_at.insert(name.to_owned(), now);
                if let Some(session) = state.sessions.get_mut(name) {
                    pose.visual = session.skeletal_pose();
                }
                if let Some(key) = prop_world_key.as_ref() {
                    let previous_revision = state
                        .prop_sent_revisions
                        .get(name)
                        .filter(|(previous_key, _)| previous_key == key)
                        .map_or(0, |(_, revision)| *revision);
                    if let Some(world) = state.prop_worlds.get(key) {
                        let (updates, revision) = world.changes_since(previous_revision);
                        prop_updates = updates;
                        state
                            .prop_sent_revisions
                            .insert(name.to_owned(), (key.clone(), revision));
                    }
                }
            }
            (
                pose,
                should_refresh_collision,
                should_broadcast,
                map_backed,
                map_filename,
                map_digests,
                acknowledged_sequence,
                prop_updates,
            )
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
        broadcast_pose(
            server,
            player.as_entity().get_id(),
            &pose,
            true,
            map_backed,
            map_filename.as_deref(),
            map_digests.as_ref(),
            acknowledged_sequence,
            &prop_updates,
        );
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

fn should_refresh_collision(map_backed: bool, center: (f32, f32, f32), root: [f32; 3]) -> bool {
    !map_backed && collision_refresh_needed(center, root)
}

fn initial_scan_recenter_needed(center: (f32, f32, f32), position: (f32, f32, f32)) -> bool {
    let dx = position.0 - center.0;
    let dy = position.1 - center.1;
    let dz = position.2 - center.2;
    dx * dx + dz * dz >= 16.0 || dy.abs() >= 1.5
}

fn broadcast_pose(
    server: &Server,
    entity_id: u32,
    pose: &SkatePose,
    active: bool,
    map_backed: bool,
    map_filename: Option<&str>,
    map_digests: Option<&MapDigests>,
    acknowledged_sequence: u32,
    prop_updates: &[prop_physics::PropUpdate],
) {
    let payload = encode_pose_payload(
        entity_id,
        pose,
        active,
        map_backed,
        map_filename,
        map_digests,
        acknowledged_sequence,
        prop_updates,
    );
    broadcast_payload(server, &payload);
}

fn encode_pose_payload(
    entity_id: u32,
    pose: &SkatePose,
    active: bool,
    map_backed: bool,
    map_filename: Option<&str>,
    map_digests: Option<&MapDigests>,
    acknowledged_sequence: u32,
    prop_updates: &[prop_physics::PropUpdate],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1100);
    payload.push(11); // version
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
    append_map_identity(&mut payload, map_backed, map_filename, map_digests);
    payload.extend_from_slice(&acknowledged_sequence.to_be_bytes());
    append_prop_updates(&mut payload, prop_updates);
    payload
}

fn append_prop_updates(payload: &mut Vec<u8>, updates: &[prop_physics::PropUpdate]) {
    let count = updates.len().min(prop_physics::MAX_PROP_UPDATES);
    payload.extend_from_slice(&(count as u16).to_be_bytes());
    for update in updates.iter().take(count) {
        payload.extend_from_slice(&update.identity.to_be_bytes());
        for value in update.translation {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&update.yaw.to_be_bytes());
        for value in update.pivot {
            payload.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn append_map_identity(
    payload: &mut Vec<u8>,
    map_backed: bool,
    map_filename: Option<&str>,
    map_digests: Option<&MapDigests>,
) {
    payload.push(u8::from(map_backed));
    let map_filename = if map_backed {
        map_filename.unwrap_or_default().as_bytes()
    } else {
        &[]
    };
    let map_filename_len = map_filename.len().min(u16::MAX as usize);
    payload.extend_from_slice(&(map_filename_len as u16).to_be_bytes());
    payload.extend_from_slice(&map_filename[..map_filename_len]);
    for digest in map_digests
        .map(|digests| [digests.map.as_str(), digests.props.as_str()])
        .unwrap_or(["", ""])
    {
        let decoded = (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| {
                (0..32)
                    .map(|index| u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16).ok())
                    .collect::<Option<Vec<_>>>()
            });
        if let Some(Some(bytes)) = decoded {
            payload.extend_from_slice(&bytes);
        } else {
            payload.resize(payload.len() + 32, 0);
        }
    }
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
    payload.push(11);
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
    payload.resize(payload.len() + 36 * 4 * 2, 0);
    append_map_identity(&mut payload, false, None, None);
    payload.extend_from_slice(&0_u32.to_be_bytes());
    append_prop_updates(&mut payload, &[]);

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
        COLLISION_REFRESH_VERTICAL_DISTANCE, INPUT_TIMEOUT, MapDigests, PendingSkateLoad,
        PropWorldKey, ServerState, append_map_identity, append_prop_updates, append_visual,
        clear_skate_state,
        collision::CollisionScan,
        collision_refresh_needed, collision_work_order, encode_pose_payload, engine,
        initial_scan_recenter_needed,
        physics::{Controls, decode_sequenced},
        player_collision_impact, prop_physics, sequence_is_newer, should_refresh_collision,
        shove_impact, shove_pressed_edge, supports_client_protocol, swept_player_collision_impact,
        take_collision_budget,
    };
    use engine::SkatePose;
    use std::time::{Duration, Instant};

    fn pose(root: [f32; 3], velocity: [f32; 3]) -> SkatePose {
        SkatePose {
            root,
            velocity,
            board: [root[0], root[1], root[2], 0.0, 0.0, 0.0, 1.0],
            trick_name: String::new(),
            sequence_score: 0.0,
            line_score: 0.0,
            total_score: 0.0,
            multiplier: 1.0,
            line_time: 0.0,
            clean: true,
            sketchy: false,
            visual: None,
        }
    }

    #[test]
    fn server_state_v11_matches_the_client_codec_fixture() {
        let mut state = pose([1.25, 2.5, -3.75], [0.125, -0.25, 0.5]);
        state.board = [4.0, 5.0, 6.0, 0.0, 0.0, 0.70710677, 0.70710677];
        state.trick_name = "Kickflip".to_string();
        state.sequence_score = 123.5;
        state.line_score = 456.25;
        state.total_score = 789.75;
        state.multiplier = 2.5;
        state.line_time = 1.25;
        state.clean = false;
        state.sketchy = true;
        let digests = MapDigests {
            map: "ab".repeat(32),
            props: "cd".repeat(32),
        };
        let updates = [prop_physics::PropUpdate {
            identity: 0x0102_0304_0506_0708,
            translation: [1.5, -2.25, 0.125],
            yaw: 0.75,
            pivot: [10.0, 64.0, -5.0],
        }];
        let bytes = encode_pose_payload(
            0x0102_0304,
            &state,
            true,
            true,
            Some("University.skate"),
            Some(&digests),
            0x1122_3344,
            &updates,
        );
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            hex,
            include_str!("../client/src/test/resources/pex-state-v11.hex").trim()
        );
    }

    #[test]
    fn client_control_v9_fixture_decodes_to_server_gameplay_inputs() {
        let hex = include_str!("../client/src/test/resources/pex-control-v9.hex").trim();
        let packet = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let (controls, sequence) = decode_sequenced(&packet).unwrap();

        assert_eq!(sequence, 0x1234_5678);
        assert_eq!(controls.forward, 64.0 / 127.0);
        assert_eq!(controls.steer, -64.0 / 127.0);
        assert_eq!(controls.right_x, -1.0);
        assert_eq!(controls.right_y, 1.0);
        assert_eq!(controls.triggers, [127, 200]);
        assert_eq!(controls.dpad, 12);
        assert!(controls.push && controls.push_left && controls.brake && controls.dismount);
        assert!(controls.session_modifier && controls.respawn && controls.shove);
    }

    #[test]
    fn fast_board_impact_knocks_down_only_inside_the_proxy_envelope() {
        let attacker = pose([0.0, 0.0, 0.0], [8.0, 0.0, 0.0]);
        let target = pose([0.5, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let impulse = player_collision_impact(&attacker, &target).unwrap();
        assert!(impulse[0] > 0.0);
        assert!(impulse[1] >= 1.8);

        assert!(
            player_collision_impact(&pose([0.5, 0.0, 0.0], [4.0, 0.0, 0.0]), &target).is_none()
        );
        assert!(player_collision_impact(&attacker, &pose([1.0, 0.0, 0.0], [0.0; 3])).is_none());
        assert!(player_collision_impact(&attacker, &pose([0.5, 2.0, 0.0], [0.0; 3])).is_none());
    }

    #[test]
    fn swept_board_impact_detects_a_fast_crossing_between_samples() {
        let attacker_previous = pose([-1.0, 0.0, 0.0], [24.0, 0.0, 0.0]);
        let attacker_current = pose([1.0, 0.0, 0.0], [24.0, 0.0, 0.0]);
        let target = pose([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);

        assert!(player_collision_impact(&attacker_current, &target).is_none());
        let impulse =
            swept_player_collision_impact(&attacker_previous, &attacker_current, &target, &target)
                .unwrap();
        assert!(impulse[0] > 0.0);
        assert!(impulse[1] >= 1.8);
    }

    #[test]
    fn swept_board_impact_rejects_vertical_misses_and_nonfinite_history() {
        let attacker_previous = pose([-1.0, 0.0, 0.0], [24.0, 0.0, 0.0]);
        let attacker_current = pose([1.0, 0.0, 0.0], [24.0, 0.0, 0.0]);
        let target_above = pose([0.0, 2.0, 0.0], [0.0; 3]);
        assert!(
            swept_player_collision_impact(
                &attacker_previous,
                &attacker_current,
                &target_above,
                &target_above,
            )
            .is_none()
        );

        let mut invalid = attacker_previous.clone();
        invalid.board[0] = f32::NAN;
        let target = pose([0.0, 0.0, 0.0], [0.0; 3]);
        assert!(
            swept_player_collision_impact(&invalid, &attacker_current, &target, &target,).is_none()
        );
    }

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
    fn shove_only_knocks_down_a_nearby_skater_in_front() {
        let attacker = SkatePose {
            root: [0.0; 3],
            velocity: [0.0; 3],
            board: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            trick_name: String::new(),
            sequence_score: 0.0,
            line_score: 0.0,
            total_score: 0.0,
            multiplier: 1.0,
            line_time: 0.0,
            clean: false,
            sketchy: false,
            visual: None,
        };
        let (impulse, _) = shove_impact(&attacker, [0.0, 0.0, 1.0]).unwrap();
        assert_eq!(impulse, [0.0, 1.25, 3.0]);
        assert!(shove_impact(&attacker, [0.0, 0.0, -1.0]).is_none());
        assert!(shove_impact(&attacker, [1.0, 0.0, 0.0]).is_none());
        assert!(shove_impact(&attacker, [0.0, 0.0, 2.0]).is_none());
        assert!(shove_impact(&attacker, [0.0, 2.0, 1.0]).is_none());
    }

    #[test]
    fn shove_action_uses_fresh_input_edges_and_recovers_after_stale_packets() {
        let pressed = Controls {
            shove: true,
            ..Controls::default()
        };
        let held = pressed;
        let released = Controls::default();
        assert!(shove_pressed_edge(pressed, released, true));
        assert!(!shove_pressed_edge(held, pressed, true));
        assert!(!shove_pressed_edge(released, pressed, true));
        assert!(shove_pressed_edge(pressed, held, false));
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
    fn authored_map_collision_is_not_replaced_with_minecraft_blocks() {
        let center = (0.0, 64.0, 0.0);
        assert!(!should_refresh_collision(true, center, [64.0, 64.0, 0.0]));
        assert!(should_refresh_collision(false, center, [8.0, 64.0, 0.0]));
    }

    #[test]
    fn map_selection_prefers_university_and_honors_safe_filename_override() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pex-skate-map-selection-{nonce}"));
        let maps = root.join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(maps.join("Mega Park.skate"), b"map").unwrap();
        std::fs::write(maps.join("University.skate"), b"map").unwrap();
        assert_eq!(
            super::default_skate_map_path(&root).unwrap(),
            Some(maps.join("University.skate"))
        );
        std::fs::create_dir_all(root.join("settings")).unwrap();
        std::fs::write(root.join("settings/default-map.txt"), "Mega Park.skate\n").unwrap();
        assert_eq!(
            super::default_skate_map_path(&root).unwrap(),
            Some(maps.join("Mega Park.skate"))
        );
        std::fs::write(root.join("settings/default-map.txt"), "../outside.skate\n").unwrap();
        assert!(super::default_skate_map_path(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
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
        assert!(!supports_client_protocol(&[7]));
        assert!(!supports_client_protocol(&[4]));
        assert!(!supports_client_protocol(&[1]));
        assert!(!supports_client_protocol(&[2]));
        assert!(!supports_client_protocol(&[CLIENT_PROTOCOL_VERSION, 0]));
        assert!(!supports_client_protocol(&[]));
    }

    #[test]
    fn control_sequences_reject_replays_and_allow_u32_wraparound() {
        assert!(sequence_is_newer(11, 10));
        assert!(!sequence_is_newer(10, 10));
        assert!(!sequence_is_newer(9, 10));
        assert!(sequence_is_newer(0, u32::MAX));
        assert!(!sequence_is_newer(u32::MAX, 0));
    }

    #[test]
    fn only_fresh_controls_consumed_by_a_step_are_acknowledged() {
        let now = Instant::now();
        let mut state = ServerState::default();
        state.control_sequences.insert("rider".into(), 7);
        state.control_updated_at.insert("rider".into(), now);
        assert_eq!(state.acknowledge_control_step("rider", 0, now), 0);
        assert_eq!(state.acknowledge_control_step("rider", 1, now), 7);

        state.control_sequences.insert("rider".into(), 8);
        state.control_updated_at.insert(
            "rider".into(),
            now - INPUT_TIMEOUT - Duration::from_millis(1),
        );
        assert_eq!(state.acknowledge_control_step("rider", 1, now), 7);
        assert_eq!(state.acknowledged_control_sequences.get("rider"), Some(&7));
    }

    #[test]
    fn authored_map_identity_carries_the_selected_filename() {
        let mut payload = Vec::new();
        append_map_identity(&mut payload, true, Some("University.skate"), None);
        assert_eq!(&payload[..3], &[1, 0, 16]);
        assert_eq!(&payload[3..19], b"University.skate");
        assert_eq!(payload.len(), 83);

        payload.clear();
        append_map_identity(&mut payload, false, Some("ignored.skate"), None);
        assert_eq!(payload.len(), 67);
        assert_eq!(&payload[..3], &[0, 0, 0]);
        assert!(payload[3..].iter().all(|byte| *byte == 0));

        payload.clear();
        let digests = super::MapDigests {
            map: "ab".repeat(32),
            props: "cd".repeat(32),
        };
        append_map_identity(&mut payload, true, Some("University.skate"), Some(&digests));
        assert_eq!(&payload[19..51], &[0xab; 32]);
        assert_eq!(&payload[51..83], &[0xcd; 32]);
    }

    #[test]
    fn shared_prop_world_identity_includes_world_map_and_props_digest() {
        let props = "ab".repeat(32);
        let key = PropWorldKey::new("world-a".into(), "University.skate", &props);
        assert_eq!(
            key,
            PropWorldKey::new("world-a".into(), "university.SKATE", &props)
        );
        assert_ne!(
            key,
            PropWorldKey::new("world-b".into(), "University.skate", &props)
        );
        assert_ne!(
            key,
            PropWorldKey::new("world-a".into(), "Community.skate", &props)
        );
        assert_ne!(
            key,
            PropWorldKey::new("world-a".into(), "University.skate", &"cd".repeat(32))
        );
    }

    #[test]
    fn prop_translation_updates_have_a_bounded_big_endian_wire_layout() {
        let mut payload = Vec::new();
        append_prop_updates(
            &mut payload,
            &[prop_physics::PropUpdate {
                identity: 0x0123_4567_89AB_CDEF,
                translation: [0.25, -1.5, 2.0],
                yaw: 0.75,
                pivot: [1.0, 2.0, 3.0],
            }],
        );
        assert_eq!(payload.len(), 38);
        assert_eq!(&payload[..2], &1_u16.to_be_bytes());
        assert_eq!(&payload[2..10], &0x0123_4567_89AB_CDEF_u64.to_be_bytes());
        assert_eq!(&payload[10..14], &0.25_f32.to_be_bytes());
        assert_eq!(&payload[14..18], &(-1.5_f32).to_be_bytes());
        assert_eq!(&payload[18..22], &2.0_f32.to_be_bytes());
        assert_eq!(&payload[22..26], &0.75_f32.to_be_bytes());
        assert_eq!(&payload[26..30], &1.0_f32.to_be_bytes());
        assert_eq!(&payload[30..34], &2.0_f32.to_be_bytes());
        assert_eq!(&payload[34..38], &3.0_f32.to_be_bytes());
    }

    #[test]
    fn prop_translation_updates_match_the_client_count_limit() {
        let updates = vec![
            prop_physics::PropUpdate {
                identity: 1,
                translation: [0.0; 3],
                yaw: 0.0,
                pivot: [0.0; 3],
            };
            8_193
        ];
        let mut payload = Vec::new();
        append_prop_updates(&mut payload, &updates);
        assert_eq!(
            u16::from_be_bytes([payload[0], payload[1]]) as usize,
            prop_physics::MAX_PROP_UPDATES
        );
        assert_eq!(
            payload.len(),
            2 + prop_physics::MAX_PROP_UPDATES * (8 + 7 * 4)
        );
    }

    #[test]
    fn skate_data_sha256_matches_the_standard_digest_vector() {
        assert_eq!(
            skate_data::sha256_digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn lifecycle_cleanup_removes_active_and_pending_player_state() {
        let mut state = ServerState::default();
        let prop_key = PropWorldKey::new("world-1".into(), "University.skate", &"AB".repeat(32));
        state
            .prop_world_keys
            .insert("rider".into(), prop_key.clone());
        state
            .prop_world_keys
            .insert("other".into(), prop_key.clone());
        state
            .prop_worlds
            .insert(prop_key.clone(), super::prop_physics::PropWorld::default());
        state
            .prop_world_advanced_at
            .insert(prop_key.clone(), Instant::now());
        state.compatible_clients.insert("rider".into());
        state.enabled.insert("rider".into());
        state.map_backed.insert("rider".into());
        state
            .map_filenames
            .insert("rider".into(), "University.skate".into());
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
        state
            .collision_poses
            .insert("rider".into(), pose([0.0, 64.0, 0.0], [0.0; 3]));

        assert!(clear_skate_state(&mut state, "rider"));
        assert!(!clear_skate_state(&mut state, "rider"));
        assert!(state.prop_worlds.contains_key(&prop_key));
        assert!(!clear_skate_state(&mut state, "other"));
        assert!(!state.prop_worlds.contains_key(&prop_key));
        assert!(!state.prop_world_advanced_at.contains_key(&prop_key));
        assert!(state.compatible_clients.contains("rider"));
        assert!(!state.enabled.contains("rider"));
        assert!(!state.map_backed.contains("rider"));
        assert!(!state.map_filenames.contains_key("rider"));
        assert!(!state.pending_loads.contains("rider"));
        assert!(!state.pending_initial_scans.contains_key("rider"));
        assert!(!state.controls.contains_key("rider"));
        assert!(!state.control_updated_at.contains_key("rider"));
        assert!(!state.engine_accumulator.contains_key("rider"));
        assert!(!state.engine_updated_at.contains_key("rider"));
        assert!(!state.last_broadcast_at.contains_key("rider"));
        assert!(!state.collision_center.contains_key("rider"));
        assert!(!state.collision_scans.contains_key("rider"));
        assert!(!state.collision_poses.contains_key("rider"));
        assert!(!state.prop_world_keys.contains_key("rider"));
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
