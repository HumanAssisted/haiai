#!/usr/bin/env bash
#
# hosted-stack-up.sh — Bring up the local hosted stack so
# `cargo test --test jacs_remote_integration -- --ignored` can be run
# against a real api with a provisioned LocalStack S3 bucket and a registered
# JACS test agent.
#
# Closes Issue 041 (JACS_DOCUMENT_STORE_PRD), Issue 058 (api was missing),
# Issue 064 (cross-repo CI lane), Issue 066 (haisdk workflow side), and Issue
# 069 (local-dev developer side): the script now drives the full SDK ↔ server
# round-trip including agent registration via the debug endpoint, so a
# developer who just cloned `~/personal/haisdk` can run the integration tests
# without a deployed dashboard `hk_…`.
#
# What this script does:
#   1. Brings up postgres, redis, LocalStack S3 (incl. bucket init), AND
#      the api itself (Issue 058 fix). The api is launched with
#      HAI_DEBUG_REGISTRATION=1 and a generated bearer token (Issue 068).
#   2. Waits for the LocalStack S3 healthcheck and the api healthcheck.
#   3. Bootstraps a JACS test agent locally (no `--register`), then POSTs the
#      agent JSON + public key PEM to `/api/v1/_debug/register-test-agent`
#      with the bearer token (Issue 069 fix; Issue 068 auth).
#   4. Prints the env vars the integration tests need.
#
# IMPORTANT — security posture:
#   The debug endpoint (`/api/v1/_debug/register-test-agent`) is a CI-only
#   seam. It is double-gated:
#     - Router-level: only mounted when HAI_DEBUG_REGISTRATION=1 AND
#       ENV is not production-shaped (api refuses to start otherwise).
#     - Request-level: requires Authorization: Bearer <token> matching
#       HAI_DEBUG_REGISTRATION_TOKEN.
#   It MUST NEVER be exposed to a production deployment. Pulumi
#   (infra/pkg/apps/api.go) panics the deploy if the env var lands on a prod
#   ConfigMap.
#
# Usage:
#   ./scripts/hosted-stack-up.sh             # bring up + print env exports
#   eval "$(./scripts/hosted-stack-up.sh)"   # also export into current shell
#
# Tear down:
#   cd ~/personal/hai/api && docker compose --profile jacsdb down

set -euo pipefail

HAI_API_DIR="${HAI_API_DIR:-${HOME}/personal/hai/api}"
HAISDK_DIR="${HAISDK_DIR:-${HOME}/personal/haisdk}"
HAI_URL="${HAI_URL:-http://localhost:8080}"
JACSDB_BUCKET="${HAI_JACSDB_BUCKET:-hai-jacsdb-test}"
HAIAI_DEV_AGENT_FILE="${HAIAI_DEV_AGENT_FILE:-${HOME}/.haiai/dev-agent.json}"

# Issue 068: bearer token gating the debug endpoint. Generated fresh each run
# unless the user supplied one in their shell env. The api reads this from
# its environment; the curl POST below sends it on Authorization: Bearer.
HAI_DEBUG_REGISTRATION_TOKEN="${HAI_DEBUG_REGISTRATION_TOKEN:-$(uuidgen 2>/dev/null || echo "tok-$(date +%s)-$RANDOM")}"
export HAI_DEBUG_REGISTRATION_TOKEN
# Issue 069: enable the CI-only debug endpoint on the api spawn. The api's
# boot-time check (api/src/routes/_debug.rs::ensure_safe_for_environment)
# refuses to start with this set on production-shaped envs, so a CI runner
# can never accidentally promote this var into a prod deploy.
export HAI_DEBUG_REGISTRATION=1

if [ ! -d "${HAI_API_DIR}" ]; then
    echo "ERROR: hai-api dir not found at ${HAI_API_DIR}" >&2
    echo "Set HAI_API_DIR=/path/to/hai/api and re-run." >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: jq is required for agent JSON extraction." >&2
    echo "  macOS:  brew install jq" >&2
    echo "  Linux:  sudo apt install jq" >&2
    exit 1
fi

echo "[1/4] docker compose --profile jacsdb up -d (postgres + redis + LocalStack + api) …" >&2
# Issue 058: bring up `api` AND its `depends_on: postgres, redis` along with
# the LocalStack S3 services. Without `api`, every `--ignored` integration
# test failed at the first `HAI_URL`-bound request.
# Issue 069: HAI_DEBUG_REGISTRATION + HAI_DEBUG_REGISTRATION_TOKEN are
# exported above so the docker-compose env interpolation picks them up.
(
    cd "${HAI_API_DIR}" \
        && docker compose --profile jacsdb up -d \
            postgres redis localstack localstack-init api
)

echo "[2/4] Waiting for LocalStack S3 healthcheck …" >&2
for i in $(seq 1 60); do
    if curl -sf "http://localhost:4566/_localstack/health" 2>/dev/null | grep -q '"s3"'; then
        echo "  LocalStack S3 ready after ${i}s" >&2
        break
    fi
    if [ "${i}" -eq 60 ]; then
        echo "ERROR: LocalStack did not become healthy in 60s." >&2
        echo "  Run \`cd ${HAI_API_DIR} && docker compose logs localstack\` to investigate." >&2
        exit 1
    fi
    sleep 1
done

# Bucket existence check — localstack-init's job is `s3 mb` but we verify
# that it actually ran. If it didn't, the api will 503 every record write.
if ! aws --endpoint-url=http://localhost:4566 s3 ls 2>/dev/null | grep -q "${JACSDB_BUCKET}"; then
    echo "WARN: bucket ${JACSDB_BUCKET} not visible. Re-running localstack-init …" >&2
    (cd "${HAI_API_DIR}" && docker compose --profile jacsdb up -d --force-recreate localstack-init)
    sleep 3
    if ! aws --endpoint-url=http://localhost:4566 s3 ls 2>/dev/null | grep -q "${JACSDB_BUCKET}"; then
        echo "ERROR: bucket ${JACSDB_BUCKET} still not visible after re-running init." >&2
        echo "  Run \`docker compose --profile jacsdb logs localstack-init\` to investigate." >&2
        exit 1
    fi
fi

# Issue 058: api was the missing piece — extended readiness window because
# Rust cold-start under `cargo run` inside a debug container can take 60–120s
# on first boot.
echo "[3/4] Probing api at ${HAI_URL}/health (up to 180s for first cold compile) …" >&2
api_ready=0
for i in $(seq 1 180); do
    if curl -sf "${HAI_URL}/health" >/dev/null 2>&1; then
        echo "  api ready after ${i}s" >&2
        api_ready=1
        break
    fi
    sleep 1
done
if [ "${api_ready}" -eq 0 ]; then
    echo "ERROR: api did not become healthy at ${HAI_URL} after 180s." >&2
    echo "  Inspect logs: cd ${HAI_API_DIR} && docker compose logs api --tail=200" >&2
    echo "  This is Issue 058's failure mode — the api container is required" >&2
    echo "  for the SDK integration tests; bring-up cannot continue without it." >&2
    exit 1
fi

# Issue 069: automate test-agent registration via the debug endpoint. Replaces
# the previous "go grab a hk_… from the dashboard" step entirely.
echo "[4/4] Bootstrapping a JACS test agent and seeding via debug endpoint …" >&2

# Probe the debug endpoint with a deliberately wrong payload to confirm the
# endpoint is mounted (HAI_DEBUG_REGISTRATION=1 reached the api). 403/4xx
# means the endpoint is up; 404 means router didn't mount; connection error
# means api isn't reachable.
probe_status=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    -H "Authorization: Bearer ${HAI_DEBUG_REGISTRATION_TOKEN}" \
    -H "Content-Type: application/json" \
    --data-raw '{"jacs_id":"probe","jacs_version":"v1","public_key_pem":"-----BEGIN PUBLIC KEY-----\nPROBE\n-----END PUBLIC KEY-----","agent_json":{}}' \
    "${HAI_URL}/api/v1/_debug/register-test-agent" 2>/dev/null || echo "000")
case "${probe_status}" in
    400|422|500)
        # Endpoint is mounted; payload was rejected on its merits — that's expected.
        echo "  debug endpoint mounted (probe returned ${probe_status} on bogus payload)" >&2
        ;;
    404)
        echo "ERROR: debug endpoint returned 404 — HAI_DEBUG_REGISTRATION=1 didn't propagate to api." >&2
        echo "  Ensure docker-compose.yml api service uses HAI_DEBUG_REGISTRATION=\${HAI_DEBUG_REGISTRATION:-0}." >&2
        echo "  Tear down and re-run: cd ${HAI_API_DIR} && docker compose --profile jacsdb down && bash hosted-stack-up.sh" >&2
        exit 1
        ;;
    403)
        echo "  debug endpoint mounted (probe rejected with 403 — token check active)" >&2
        ;;
    503)
        echo "ERROR: debug endpoint returned 503 — HAI_DEBUG_REGISTRATION_TOKEN didn't reach the api." >&2
        echo "  Token in shell: ${HAI_DEBUG_REGISTRATION_TOKEN:0:8}…" >&2
        echo "  Tear down and re-run: cd ${HAI_API_DIR} && docker compose --profile jacsdb down && bash hosted-stack-up.sh" >&2
        exit 1
        ;;
    000)
        echo "ERROR: api unreachable when probing debug endpoint." >&2
        exit 1
        ;;
    201)
        # Unexpected — bogus PEM somehow accepted. Log but continue.
        echo "  WARN: debug endpoint accepted bogus probe payload (201). Continuing." >&2
        ;;
    *)
        echo "  WARN: debug endpoint probe returned ${probe_status}; continuing anyway." >&2
        ;;
esac

# Generate a fresh local agent. We use the haiai CLI built from the haisdk
# repo. `--register false` means we do NOT call the production registration
# path — we'll seed via the debug endpoint instead.
HAIAI_DATA_DIR="${HAIAI_DATA_DIR:-${HOME}/.haiai/data}"
HAIAI_KEY_DIR="${HAIAI_KEY_DIR:-${HOME}/.haiai/keys}"
HAIAI_CONFIG_PATH="${HAIAI_CONFIG_PATH:-${HOME}/.haiai/jacs.config.json}"
mkdir -p "$(dirname "${HAIAI_CONFIG_PATH}")"
mkdir -p "${HAIAI_DATA_DIR}"
mkdir -p "${HAIAI_KEY_DIR}"

# Reuse a cached agent JSON if present — the debug endpoint is idempotent
# (UPSERT on agent_id + jacs_version), so re-seeding is safe and fast.
if [ -s "${HAIAI_CONFIG_PATH}" ] && [ -s "${HAIAI_DEV_AGENT_FILE}" ]; then
    echo "  Reusing cached dev agent at ${HAIAI_DEV_AGENT_FILE}" >&2
else
    echo "  Generating fresh JACS agent via haiai CLI …" >&2
    if [ ! -d "${HAISDK_DIR}/rust" ]; then
        echo "ERROR: haisdk Rust workspace not found at ${HAISDK_DIR}/rust" >&2
        echo "  Set HAISDK_DIR=/path/to/haisdk and re-run, or generate an agent manually:" >&2
        echo "    cd <haisdk>/rust && cargo run --bin haiai --features jacs-crate -- \\" >&2
        echo "      init --name dev --register false --data-dir ${HAIAI_DATA_DIR} \\" >&2
        echo "      --key-dir ${HAIAI_KEY_DIR} --config-path ${HAIAI_CONFIG_PATH}" >&2
        exit 1
    fi
    (
        cd "${HAISDK_DIR}/rust" \
            && cargo run --quiet --bin haiai --features jacs-crate -- \
                init \
                --name "$(whoami)-localdev" \
                --register false \
                --data-dir "${HAIAI_DATA_DIR}" \
                --key-dir "${HAIAI_KEY_DIR}" \
                --config-path "${HAIAI_CONFIG_PATH}"
    )
fi

# Read the generated agent JSON + public key PEM, then POST to debug endpoint.
JACS_ID=$(jq -r '.jacs_id // .jacsId // empty' "${HAIAI_CONFIG_PATH}")
JACS_VERSION=$(jq -r '.jacs_version // .jacsVersion // "1.0.0"' "${HAIAI_CONFIG_PATH}")
if [ -z "${JACS_ID}" ]; then
    echo "ERROR: ${HAIAI_CONFIG_PATH} missing jacs_id field after init." >&2
    exit 1
fi

# Locate agent JSON file (haiai writes to {data_dir}/agents/{jacs_id}.json or similar).
AGENT_JSON_FILE=""
for candidate in \
    "${HAIAI_DATA_DIR}/agents/${JACS_ID}.json" \
    "${HAIAI_DATA_DIR}/${JACS_ID}.json" \
    "${HAIAI_DATA_DIR}/agent.json"; do
    if [ -s "${candidate}" ]; then
        AGENT_JSON_FILE="${candidate}"
        break
    fi
done
if [ -z "${AGENT_JSON_FILE}" ]; then
    echo "ERROR: could not locate agent JSON file under ${HAIAI_DATA_DIR}." >&2
    echo "  Looked for: agents/${JACS_ID}.json, ${JACS_ID}.json, agent.json" >&2
    exit 1
fi
AGENT_JSON=$(cat "${AGENT_JSON_FILE}")

# Locate public key PEM. haiai CLI writes one or more PEM files to KEY_DIR.
PUBLIC_KEY_FILE=""
for candidate in "${HAIAI_KEY_DIR}/${JACS_ID}.public.pem" \
                  "${HAIAI_KEY_DIR}/jacs.public.pem"; do
    if [ -s "${candidate}" ]; then
        PUBLIC_KEY_FILE="${candidate}"
        break
    fi
done
if [ -z "${PUBLIC_KEY_FILE}" ]; then
    PUBLIC_KEY_FILE=$(find "${HAIAI_KEY_DIR}" -maxdepth 2 -name '*.public.pem' -print 2>/dev/null | head -1)
fi
if [ -z "${PUBLIC_KEY_FILE}" ] || [ ! -s "${PUBLIC_KEY_FILE}" ]; then
    echo "ERROR: could not locate a public PEM under ${HAIAI_KEY_DIR}." >&2
    exit 1
fi
PUBLIC_KEY_PEM=$(cat "${PUBLIC_KEY_FILE}")

echo "  POSTing seed for jacs_id=${JACS_ID} version=${JACS_VERSION} …" >&2
SEED_PAYLOAD=$(jq -n \
    --arg jacs_id "${JACS_ID}" \
    --arg jacs_version "${JACS_VERSION}" \
    --arg public_key_pem "${PUBLIC_KEY_PEM}" \
    --argjson agent_json "${AGENT_JSON}" \
    '{jacs_id: $jacs_id, jacs_version: $jacs_version, public_key_pem: $public_key_pem, agent_json: $agent_json}')

seed_status=$(curl -sf -o /tmp/hai-debug-seed-out -w "%{http_code}" -X POST \
    -H "Authorization: Bearer ${HAI_DEBUG_REGISTRATION_TOKEN}" \
    -H "Content-Type: application/json" \
    --data-raw "${SEED_PAYLOAD}" \
    "${HAI_URL}/api/v1/_debug/register-test-agent" 2>/dev/null || echo "000")
if [ "${seed_status}" != "201" ]; then
    echo "ERROR: debug seed failed (status=${seed_status})." >&2
    if [ -s /tmp/hai-debug-seed-out ]; then
        echo "Body:" >&2
        cat /tmp/hai-debug-seed-out >&2
        echo >&2
    fi
    exit 1
fi
echo "  Registered local agent ${JACS_ID} via debug endpoint (201 Created)." >&2

# Cache the agent file so subsequent runs reuse the same identity.
cp "${AGENT_JSON_FILE}" "${HAIAI_DEV_AGENT_FILE}"

cat <<EOF >&2

----- Stack ready -----

The hosted stack is up AND a JACS test agent has been auto-registered. You
can now run the haisdk integration tests directly:

  cd ${HAISDK_DIR}/rust && \\
    cargo test -p haiai --features jacs-crate \\
      --test jacs_remote_integration -- --ignored

To export the env into your current shell, run:

  eval "\$(${0})"

EOF

# These exports go to STDOUT so \`eval "\$(./scripts/hosted-stack-up.sh)"\`
# imports them into the calling shell.
echo "export HAI_JACS_REMOTE_TEST_URL=${HAI_URL}"
echo "export HAI_URL=${HAI_URL}"
echo "export HAI_JACSDB_BUCKET=${JACSDB_BUCKET}"
echo "export AWS_ENDPOINT_URL=http://localhost:4566"
echo "export AWS_ACCESS_KEY_ID=test"
echo "export AWS_SECRET_ACCESS_KEY=test"
echo "export AWS_DEFAULT_REGION=us-east-1"
echo "export HAIAI_DEV_AGENT_FILE=${HAIAI_DEV_AGENT_FILE}"
echo "export HAIAI_CONFIG_PATH=${HAIAI_CONFIG_PATH}"
echo "export HAI_DEBUG_REGISTRATION_TOKEN=${HAI_DEBUG_REGISTRATION_TOKEN}"
