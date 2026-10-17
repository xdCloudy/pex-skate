#!/usr/bin/env bash
set -euo pipefail

plugin=${1:?usage: smoke-test-pex-runtime.sh <pex-skate.wasm> [converted-data-folder]}
converted_data=${2:-}
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
server_version=v1.0.0
server_sha256=198ec31b4fb96ac51970ca5366691da2b44add2954ab55ee0bda446423ed50ba
server_name=pumpkin-X64-Linux
server_url="https://github.com/Conman1533/PEX/releases/download/${server_version}/${server_name}"
spawn_protection=${PEX_SKATE_SMOKE_SPAWN_PROTECTION:-16}
if [[ ! "$spawn_protection" =~ ^[0-9]+$ ]]; then
  echo "PEX_SKATE_SMOKE_SPAWN_PROTECTION must be a non-negative integer" >&2
  exit 2
fi
smoke_players=${PEX_SKATE_SMOKE_PLAYERS:-1}
if [[ ! "$smoke_players" =~ ^[0-9]+$ ]] || (( 10#$smoke_players < 1 || 10#$smoke_players > 4 )); then
  echo "PEX_SKATE_SMOKE_PLAYERS must be an integer from 1 through 4" >&2
  exit 2
fi
work_dir=$(mktemp -d)
server_pid=

stop_server() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -INT "$server_pid" 2>/dev/null || true
    for _ in $(seq 1 15); do
      kill -0 "$server_pid" 2>/dev/null || break
      sleep 1
    done
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  server_pid=
}

cleanup() {
  stop_server
  rm -rf "$work_dir"
}
trap cleanup EXIT

private_source=
if [[ -n "$converted_data" ]]; then
  if [[ ! -d "$converted_data" ]]; then
    echo "Converted data folder does not exist: $converted_data" >&2
    exit 1
  fi
  if [[ -d "$converted_data/assets/private" ]]; then
    private_source="$converted_data/assets/private"
  elif [[ -d "$converted_data/private" ]]; then
    private_source="$converted_data/private"
  else
    echo "Converted data folder needs assets/private/ or private/" >&2
    exit 1
  fi
  if [[ ! -f "$private_source/stock/data/config/input.cfg" ]]; then
    echo "Converted data folder is missing the Skate engine files under stock/data; a client-only config/pex-skate folder is not sufficient." >&2
    exit 1
  fi
fi

curl --fail --location --retry 3 --silent --show-error "$server_url" -o "$work_dir/$server_name"
printf '%s  %s\n' "$server_sha256" "$work_dir/$server_name" | sha256sum --check --status
chmod +x "$work_dir/$server_name"

mkdir -p "$work_dir/plugins"
cp "$plugin" "$work_dir/plugins/pex-skate.wasm"
if [[ -n "$converted_data" ]]; then
  data_root="$work_dir/plugins/data/pex-skate"
  mkdir -p "$data_root/private"
  cp -a "$private_source/." "$data_root/private/"
  optional_data=(maps props settings maps.json)
  if [[ "${PEX_SKATE_SMOKE_MAP:-}" == "minecraft" ]]; then
    optional_data=(settings)
  fi
  for entry in "${optional_data[@]}"; do
    if [[ -e "$converted_data/$entry" ]]; then
      cp -a "$converted_data/$entry" "$data_root/"
    fi
  done
fi
plugin_sha256=$(sha256sum "$work_dir/plugins/pex-skate.wasm" | cut -d ' ' -f 1)
printf '{"entries":{"%s":{"permissions_requested":["fs.read.data"],"approved":true}}}\n' \
  "$plugin_sha256" > "$work_dir/plugins/permission_cache.json"
cat > "$work_dir/pumpkin.toml" <<EOF
spawn_protection = $spawn_protection

[networking.java]
online_mode = false
encryption = false

[networking.java.authentication]
enabled = false
EOF

cd "$work_dir"

start_server() {
  "$work_dir/$server_name" > server.log 2>&1 &
  server_pid=$!

  local ready=false
  for _ in $(seq 1 180); do
    if ! kill -0 "$server_pid" 2>/dev/null; then
      tail -n 40 server.log
      echo "PEX exited before the skate plugin became available" >&2
      return 1
    fi
    if grep -Eqi 'permission denied|failed to load plugin|plugin.*(failed|error|panic)' server.log; then
      tail -n 40 server.log
      echo "PEX reported a plugin load failure" >&2
      return 1
    fi
    if grep -q 'uses the following permissions: \["fs.read.data"\]' server.log \
        && python3 "$script_dir/check_pex_status.py" --quiet; then
      ready=true
      break
    fi
    sleep 1
  done

  if [[ "$ready" != true ]]; then
    tail -n 40 server.log
    echo "PEX did not reach the listening state with pex-skate loaded" >&2
    return 1
  fi

  local module_cache
  module_cache=$(find plugins/cache/modules -type f -print -quit 2>/dev/null || true)
  if [[ -z "$module_cache" ]]; then
    tail -n 40 server.log
    echo "PEX did not compile the pex-skate WebAssembly component" >&2
    return 1
  fi
}

run_smoke_players() {
  local exercise_skate=$1
  local exercise_timeout=${2:-}
  local player_index
  local player_pid
  local player_failed=0
  local -a player_pids=()
  for ((player_index = 1; player_index <= smoke_players; player_index++)); do
    local -a player_args=(--username "PexSkateSmoke${player_index}")
    if [[ "$exercise_skate" == true ]]; then
      player_args+=(--exercise-skate --exercise-timeout "$exercise_timeout"
        --exercise-duration "${PEX_SKATE_SMOKE_GAMEPLAY_DURATION:-3}"
        --asset-root "$data_root/private")
    fi
    python3 "$script_dir/test_pex_payload_client.py" "${player_args[@]}" &
    player_pids+=("$!")
  done
  for player_pid in "${player_pids[@]}"; do
    if ! wait "$player_pid"; then
      player_failed=1
    fi
  done
  return "$player_failed"
}

run_server_smoke() {
  local map_name=${1:-}
  if [[ -n "$map_name" ]]; then
    mkdir -p "$data_root/settings"
    printf '%s\n' "$map_name" > "$data_root/settings/default-map.txt"
  fi

  start_server
  if [[ -n "$converted_data" ]]; then
    local exercise_timeout=${PEX_SKATE_SMOKE_TIMEOUT_SECONDS:-180}
    if ! run_smoke_players true "$exercise_timeout"; then
      grep -E 'MAP_READ_TIMING|IW4L_SKATE_LOAD|SKATE_PHYSICS_MODE|SKATE_GRIND_READY|Can.t keep up' server.log | tail -n 60 || true
      tail -n 12 server.log | cut -c1-1200
      return 1
    fi
    echo "Skate data/session initialization timings:"
    grep -E 'MAP_READ_TIMING|IW4L_SKATE_LOAD|SKATE_PHYSICS_MODE|SKATE_GRIND_READY|Can.t keep up' server.log | tail -n 60 || true
    python3 "$script_dir/check_pex_status.py"
    echo "PEX $server_version loaded ${map_name:-the default Skate map}, activated $smoke_players engine session(s), consumed client controls, and passed the status handshake."
  else
    if ! run_smoke_players false; then
      tail -n 40 server.log
      return 1
    fi
    python3 "$script_dir/check_pex_status.py"
    echo "PEX $server_version loaded pex-skate.wasm, accepted $smoke_players offline Minecraft 26.3 player(s), acknowledged the PEX Skate payload, and passed the status handshake."
  fi
  if [[ "${PEX_SKATE_SMOKE_KEEP_SERVER:-0}" == "1" ]]; then
    echo "Leaving the disposable PEX server running for a manual client check (pid $server_pid); press Ctrl+C to stop it."
    wait "$server_pid"
  else
    stop_server
  fi
}

if [[ -n "$converted_data" ]]; then
  if [[ -n "${PEX_SKATE_SMOKE_MAPS:-}" ]]; then
    IFS=';' read -r -a smoke_maps <<< "$PEX_SKATE_SMOKE_MAPS"
    for map_name in "${smoke_maps[@]}"; do
      if [[ -z "$map_name" ]]; then
        echo "PEX_SKATE_SMOKE_MAPS contains an empty map name" >&2
        exit 1
      fi
      run_server_smoke "$map_name"
    done
  else
    run_server_smoke "${PEX_SKATE_SMOKE_MAP:-}"
  fi
else
  run_server_smoke
fi
