#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
state_dir="${GOOSE_SONAR_SMOKE_HOME:-${HOME}/.local/share/goose/sonar-smoke}"
goose_bin="${repo_root}/target/release/goose"
bridge_bin="${script_dir}/target/release/goose-sonar-bridge"
controller=""
session_id=""
build=true
relays=()
relay_count=0

usage() {
    printf '%s\n' \
        "Usage: $0 --controller NPUB [options]" \
        "" \
        "Options:" \
        "  --controller NPUB   Sonar identity allowed to invite and control goose" \
        "  --session-id ID     Pair to an existing goose session" \
        "  --relay URL         Nostr relay URL; repeat for multiple relays" \
        "  --state-dir PATH    Persistent bridge state directory" \
        "  --goose-bin PATH    Feature-enabled goose release binary" \
        "  --bridge-bin PATH   goose-sonar-bridge release binary" \
        "  --no-build          Use existing binaries instead of building" \
        "  -h, --help          Show this help"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --controller)
            controller="${2:?missing controller npub}"
            shift 2
            ;;
        --session-id)
            session_id="${2:?missing session id}"
            shift 2
            ;;
        --relay)
            relays[relay_count]="${2:?missing relay URL}"
            relay_count=$((relay_count + 1))
            shift 2
            ;;
        --state-dir)
            state_dir="${2:?missing state directory}"
            shift 2
            ;;
        --goose-bin)
            goose_bin="${2:?missing goose binary path}"
            shift 2
            ;;
        --bridge-bin)
            bridge_bin="${2:?missing bridge binary path}"
            shift 2
            ;;
        --no-build)
            build=false
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'Unknown option: %s\n\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ ! "${controller}" =~ ^npub1 ]]; then
    printf '%s\n' "--controller must be an npub1 identity" >&2
    exit 2
fi

if [[ "${build}" == true ]]; then
    cargo build --release --manifest-path "${script_dir}/Cargo.toml"
    cargo build --release -p goose-cli --features sonar-gateway --manifest-path "${repo_root}/Cargo.toml"
fi

for binary in "${goose_bin}" "${bridge_bin}"; do
    if [[ ! -x "${binary}" ]]; then
        printf 'Missing executable: %s\n' "${binary}" >&2
        exit 1
    fi
done

bridge_npub="$("${bridge_bin}" --home "${state_dir}" identity)"
printf '\nBridge identity: %s\n' "${bridge_npub}"
printf '%s\n' \
    "1. In Sonar, use controller ${controller} to invite this bridge identity to a group." \
    "2. Wait for the invite to publish, then press Enter here."
read -r

log_file="${TMPDIR:-/tmp}/goose-sonar-smoke-$$.log"
gateway_pid=""
tail_pid=""

# shellcheck disable=SC2329
cleanup() {
    if [[ -n "${tail_pid}" ]] && kill -0 "${tail_pid}" 2>/dev/null; then
        kill "${tail_pid}" 2>/dev/null || true
    fi
    if [[ -n "${gateway_pid}" ]] && kill -0 "${gateway_pid}" 2>/dev/null; then
        kill "${gateway_pid}" 2>/dev/null || true
        wait "${gateway_pid}" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

gateway_args=(
    gateway start sonar
    --no-persist
    --bridge-path "${bridge_bin}"
    --sonar-home "${state_dir}"
    --controller "${controller}"
)
if ((relay_count > 0)); then
    for ((index = 0; index < relay_count; index++)); do
        gateway_args+=(--relay "${relays[index]}")
    done
fi

"${goose_bin}" "${gateway_args[@]}" >"${log_file}" 2>&1 &
gateway_pid=$!

sleep 2
if ! kill -0 "${gateway_pid}" 2>/dev/null; then
    printf 'Gateway exited during startup. Log: %s\n' "${log_file}" >&2
    sed -n '1,160p' "${log_file}" >&2
    exit 1
fi

pair_args=(gateway pair sonar)
if [[ -n "${session_id}" ]]; then
    pair_args+=(--session-id "${session_id}")
fi
pairing_output="$("${goose_bin}" "${pair_args[@]}")"

printf '\n%s\n\n' "${pairing_output}"
printf '%s\n' \
    "Wait for the group to receive the ready message, then:" \
    "1. Send the pairing code above from ${controller}." \
    "2. Send: Reply with exactly SONAR_SMOKE_OK" \
    "3. Verify the group receives SONAR_SMOKE_OK once." \
    "4. Send two numbered prompts quickly and verify replies stay in order." \
    "5. Send a prompt from a non-controller member and verify goose ignores it." \
    "" \
    "Press Ctrl+C to stop. Gateway log: ${log_file}"

tail -f "${log_file}" &
tail_pid=$!
set +e
wait "${gateway_pid}"
gateway_status=$?
set -e
exit "${gateway_status}"
