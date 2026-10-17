#!/usr/bin/env bash
set -euo pipefail

plugin=${1:?usage: smoke-test-pex-runtime.sh <pex-skate.wasm> [converted-data-folder]}
converted_data=${2:-}
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
server_version=v1.0.0
server_sha256=198ec31b4fb96ac51970ca5366691da2b44add2954ab55ee0bda446423ed50ba
server_name=pumpkin-X64-Linux
server_url="https://github.com/Conman1533/PEX/releases/download/${server_version}/${server_name}"
work_dir=$(mktemp -d)
server_pid=

cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -INT "$server_pid" 2>/dev/null || true
    for _ in $(seq 1 15); do
      kill -0 "$server_pid" 2>/dev/null || break
      sleep 1
    done
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$work_dir"
}
trap cleanup EXIT

curl --fail --location --retry 3 --silent --show-error "$server_url" -o "$work_dir/$server_name"
printf '%s  %s\n' "$server_sha256" "$work_dir/$server_name" | sha256sum --check --status
chmod +x "$work_dir/$server_name"

mkdir -p "$work_dir/plugins"
cp "$plugin" "$work_dir/plugins/pex-skate.wasm"
if [[ -n "$converted_data" ]]; then
  if [[ ! -d "$converted_data" ]]; then
    echo "Converted data folder does not exist: $converted_data" >&2
    exit 1
  fi
  data_root="$work_dir/plugins/data/pex-skate"
  if [[ -d "$converted_data/assets/private" ]]; then
    private_source="$converted_data/assets/private"
  elif [[ -d "$converted_data/private" ]]; then
    private_source="$converted_data/private"
  else
    echo "Converted data folder needs assets/private/ or private/" >&2
    exit 1
  fi
  mkdir -p "$data_root/private"
  cp -a "$private_source/." "$data_root/private/"
  for entry in maps props settings maps.json; do
    if [[ -e "$converted_data/$entry" ]]; then
      cp -a "$converted_data/$entry" "$data_root/"
    fi
  done
  if [[ -n "${PEX_SKATE_SMOKE_MAP:-}" ]]; then
    mkdir -p "$data_root/settings"
    printf '%s\n' "$PEX_SKATE_SMOKE_MAP" > "$data_root/settings/default-map.txt"
  fi
fi
plugin_sha256=$(sha256sum "$work_dir/plugins/pex-skate.wasm" | cut -d ' ' -f 1)
printf '{"entries":{"%s":{"permissions_requested":["fs.read.data"],"approved":true}}}\n' \
  "$plugin_sha256" > "$work_dir/plugins/permission_cache.json"
cat > "$work_dir/pumpkin.toml" <<'EOF'
[networking.java]
online_mode = false
encryption = false

[networking.java.authentication]
enabled = false
EOF

cd "$work_dir"
"$work_dir/$server_name" > server.log 2>&1 &
server_pid=$!

ready=false
for _ in $(seq 1 180); do
  if ! kill -0 "$server_pid" 2>/dev/null; then
    cat server.log
    echo "PEX exited before the skate plugin became available" >&2
    exit 1
  fi
  if grep -Eqi 'permission denied|failed to load plugin|plugin.*(failed|error|panic)' server.log; then
    cat server.log
    echo "PEX reported a plugin load failure" >&2
    exit 1
  fi
  if grep -q 'uses the following permissions: \["fs.read.data"\]' server.log \
      && python3 "$script_dir/check_pex_status.py" --quiet; then
    ready=true
    break
  fi
  sleep 1
done

if [[ "$ready" != true ]]; then
  cat server.log
  echo "PEX did not reach the listening state with pex-skate loaded" >&2
  exit 1
fi

module_cache=$(find plugins/cache/modules -type f -print -quit 2>/dev/null || true)
if [[ -z "$module_cache" ]]; then
  cat server.log
  echo "PEX did not compile the pex-skate WebAssembly component" >&2
  exit 1
fi

if [[ -n "$converted_data" ]]; then
  exercise_timeout=${PEX_SKATE_SMOKE_TIMEOUT_SECONDS:-180}
  if ! python3 "$script_dir/test_pex_payload_client.py" --exercise-skate --exercise-timeout "$exercise_timeout"; then
    tail -n 12 server.log | cut -c1-1200
    exit 1
  fi
else
  if ! python3 "$script_dir/test_pex_payload_client.py"; then
    cat server.log
    exit 1
  fi
fi
python3 "$script_dir/check_pex_status.py"
if [[ -n "$converted_data" ]]; then
  echo "PEX $server_version loaded Skate data, activated an engine session, consumed client controls, and passed the status handshake."
else
  echo "PEX $server_version loaded pex-skate.wasm, accepted an offline Minecraft 26.3 player, acknowledged the PEX Skate payload, and passed the status handshake."
fi
