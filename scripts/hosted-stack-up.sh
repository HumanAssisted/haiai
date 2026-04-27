#!/usr/bin/env bash
#
# hosted-stack-up.sh — Bring up the local hosted stack so
# `cargo test --test jacs_remote_integration -- --ignored` can be run
# against a real api with a provisioned LocalStack S3 bucket.
#
# Closes Issue 041 (JACS_DOCUMENT_STORE_PRD): the previous
# `HOSTED_STACK_LOCAL.md` walk-through was a markdown file that
# referenced but did not actually establish the live preconditions.
# This script is the runnable replacement for the bring-up half.
#
# What this script does:
#   1. Brings up postgres, redis, LocalStack S3, and the api via the
#      `--profile jacsdb` docker-compose stack in the hai repo.
#   2. Provisions the JACS Document Store bucket via localstack-init.
#   3. Waits for the api healthcheck.
#   4. Prints the env vars the integration tests need.
#
# What this script does NOT do (out of scope — registration requires an
# SSO-issued one-time `hk_…` key from the dashboard):
#   - Register a test agent via `haiai init --register --key hk_…`.
#     The user runs that step interactively after the stack is up:
#     `cd <haisdk>/rust && cargo run --bin haiai -- init --name <name> --key <hk_…> --register`
#
# Usage:
#   ./scripts/hosted-stack-up.sh             # bring up + print env exports
#   eval "$(./scripts/hosted-stack-up.sh)"   # also export into current shell
#
# Tear down:
#   cd ~/personal/hai/api && docker compose --profile jacsdb down

set -euo pipefail

HAI_API_DIR="${HAI_API_DIR:-${HOME}/personal/hai/api}"
HAI_URL="${HAI_URL:-http://localhost:8080}"
JACSDB_BUCKET="${HAI_JACSDB_BUCKET:-hai-jacsdb-test}"

if [ ! -d "${HAI_API_DIR}" ]; then
    echo "ERROR: hai-api dir not found at ${HAI_API_DIR}" >&2
    echo "Set HAI_API_DIR=/path/to/hai/api and re-run." >&2
    exit 1
fi

echo "[1/3] docker compose --profile jacsdb up -d (LocalStack S3 + bucket + api) …" >&2
(cd "${HAI_API_DIR}" && docker compose --profile jacsdb up -d localstack localstack-init)

echo "[2/3] Waiting for LocalStack S3 healthcheck …" >&2
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

echo "[3/3] Probing api at ${HAI_URL}/health …" >&2
api_ready=0
for i in $(seq 1 60); do
    if curl -sf "${HAI_URL}/health" >/dev/null 2>&1; then
        echo "  api ready after ${i}s" >&2
        api_ready=1
        break
    fi
    sleep 1
done
if [ "${api_ready}" -eq 0 ]; then
    echo "WARN: api did not become healthy at ${HAI_URL}." >&2
    echo "  This script does NOT bring up the api container — only the LocalStack" >&2
    echo "  prerequisites. Bring up the api separately:" >&2
    echo "    cd ${HAI_API_DIR} && docker compose --profile jacsdb up -d api" >&2
    echo "  …or run it from your IDE / cargo against ${HAI_API_DIR}/src." >&2
fi

cat <<EOF >&2

----- Stack ready -----

Next steps for the SDK integration tests:

1. Register an agent (one-time, requires hk_… from the dashboard):
     cd <haisdk>/rust && \\
       cargo run --bin haiai --features jacs-crate -- \\
       init --name <name> --key <hk_…> --register

2. Export the test env (or eval this script's stdout):
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

cat <<'EOF' >&2

3. Run the integration tests:
     cd <haisdk>/rust && \
       cargo test -p haiai --features jacs-crate \
         --test jacs_remote_integration -- --ignored

EOF
