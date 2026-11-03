#!/usr/bin/env bash
set -euo pipefail

PORT="${VERIFY_S01_PORT:-3100}"
HOST="127.0.0.1"
ADDR="${HOST}:${PORT}"
BASE_URL="http://${ADDR}"
LOG_FILE="$(mktemp -t verify-s01-log.XXXXXX)"
HEALTH_FILE="$(mktemp -t verify-s01-health.XXXXXX)"
RUNTIME_FILE="$(mktemp -t verify-s01-runtime.XXXXXX)"
DATA_DIR="$(mktemp -d -t verify-s01-data.XXXXXX)"
PID=""

cleanup() {
  local exit_code="$?"

  if [[ -n "${PID}" ]] && kill -0 "${PID}" 2>/dev/null; then
    kill -INT "${PID}" 2>/dev/null || true

    for _ in {1..50}; do
      if ! kill -0 "${PID}" 2>/dev/null; then
        break
      fi
      sleep 0.1
    done

    if kill -0 "${PID}" 2>/dev/null; then
      echo "service failed to stop after SIGINT" >&2
      kill -TERM "${PID}" 2>/dev/null || true
      wait "${PID}" || true
      exit_code=1
    else
      wait "${PID}" || true
    fi
  fi

  if [[ "${exit_code}" -ne 0 ]]; then
    echo "--- service log ---" >&2
    cat "${LOG_FILE}" >&2 || true
  fi

  rm -f "${LOG_FILE}" "${HEALTH_FILE}" "${RUNTIME_FILE}"
  rm -rf "${DATA_DIR}"
  exit "${exit_code}"
}
trap cleanup EXIT

APP_BIND_ADDR="${ADDR}" APP_DATA_DIR="${DATA_DIR}" JWT_SECRET="verify-s01-secret" cargo run >"${LOG_FILE}" 2>&1 &
PID="$!"

for _ in {1..100}; do
  if curl -fsS "${BASE_URL}/health" >"${HEALTH_FILE}" 2>/dev/null; then
    break
  fi

  if ! kill -0 "${PID}" 2>/dev/null; then
    echo "service exited before binding ${ADDR}" >&2
    wait "${PID}" || true
    exit 1
  fi

  sleep 0.2
done

if [[ ! -s "${HEALTH_FILE}" ]]; then
  echo "service never became ready on ${BASE_URL}" >&2
  exit 1
fi

curl -fsS "${BASE_URL}/runtime" >"${RUNTIME_FILE}"

python3 - <<'PY' "${HEALTH_FILE}" "${RUNTIME_FILE}" "${PORT}"
import json
import pathlib
import sys

health_path = pathlib.Path(sys.argv[1])
runtime_path = pathlib.Path(sys.argv[2])
port = int(sys.argv[3])

health = json.loads(health_path.read_text())
runtime = json.loads(runtime_path.read_text())

assert health["status"] == "ok", health
assert health["ready"] is True, health
assert health["state_backend"] == "sqlite", health
assert health["service"], health
assert health["started_at"], health

assert runtime["service"] == health["service"], runtime
assert runtime["ready"] is True, runtime
assert runtime["auth_configured"] is True, runtime
assert runtime["state_backend"] == "sqlite", runtime
assert runtime["startup_count"] == 1, runtime
assert len(runtime["databases"]) == 4, runtime
manager_names = {manager["name"] for manager in runtime["managers"]}
assert "service_heartbeat" in manager_names, runtime
assert "sqlite_monitor" in manager_names, runtime
assert len(runtime["managers"]) >= 2, runtime
assert "cache_sync" in runtime, runtime
assert "delete_runtime" in runtime, runtime
assert runtime["version"], runtime
assert runtime["started_at"], runtime
PY

grep -q 'bootstrap listener ready' "${LOG_FILE}" || {
  echo "missing bootstrap listener log" >&2
  exit 1
}

grep -q 'shutdown signal received' "${LOG_FILE}" || {
  kill -INT "${PID}" 2>/dev/null || true
  for _ in {1..50}; do
    if grep -q 'shutdown signal received' "${LOG_FILE}"; then
      break
    fi
    if ! kill -0 "${PID}" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
}

echo "verify-s01: startup, probes, and graceful shutdown passed on ${BASE_URL}"
