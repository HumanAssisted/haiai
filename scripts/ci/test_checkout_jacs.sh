#!/usr/bin/env bash
# Hermetic checkout checks: local Git objects only, no network or toolchain builds.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/haiai-jacs-checkout.XXXXXX")"
trap 'rm -rf -- "$test_root"' EXIT
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_ALLOW_PROTOCOL=file GIT_CONFIG_COUNT=0
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_COMMON_DIR GIT_CONFIG_PARAMETERS
export GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.test
export GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.test

expected_version="$(awk '/^jacs = / {split($0, fields, "\""); sub(/^=/, "", fields[2]); print fields[2]}' "$root/rust/haiai/Cargo.toml")"
repository="$test_root/source"
git init --quiet "$repository"
mkdir "$repository/jacs"
printf '[package]\nname = "jacs"\nversion = "%s"\n' "$expected_version" > "$repository/jacs/Cargo.toml"
git -C "$repository" add jacs/Cargo.toml
git -C "$repository" -c commit.gpgsign=false commit --quiet -m reviewed
reviewed="$(git -C "$repository" rev-parse HEAD)"
git -C "$repository" -c tag.gpgsign=false tag -a "crate/v$expected_version" -m reviewed
printf '[package]\nname = "jacs"\nversion = "99.99.99"\n' > "$repository/jacs/Cargo.toml"
git -C "$repository" add jacs/Cargo.toml
git -C "$repository" -c commit.gpgsign=false commit --quiet -m newer-unreviewed
newer="$(git -C "$repository" rev-parse HEAD)"
# A branch with the canonical tag's spelling must never shadow the tag.
git -C "$repository" branch "crate/v$expected_version" "$newer"

checkout() {
  bash "$root/scripts/ci/checkout_jacs.sh" "$1" "$2" "$repository"
}

expect_failure() {
  if "$@" > "$test_root/last-failure.log" 2>&1; then
    echo "Expected command to fail: $*" >&2
    exit 1
  fi
}

checkout "$reviewed" "$test_root/by-sha"
[[ "$(git -C "$test_root/by-sha" rev-parse HEAD)" == "$reviewed" ]]
expect_failure git -C "$test_root/by-sha" symbolic-ref -q HEAD

checkout "crate/v$expected_version" "$test_root/by-tag"
[[ "$(git -C "$test_root/by-tag" rev-parse HEAD)" == "$reviewed" ]]
expect_failure git -C "$test_root/by-tag" symbolic-ref -q HEAD
git -C "$repository" tag crate/v99.99.99 "$reviewed"
expect_failure checkout crate/v99.99.99 "$test_root/mislabeled-tag"
[[ ! -e "$test_root/mislabeled-tag" ]]
git -C "$repository" tag -d "crate/v$expected_version" >/dev/null
expect_failure checkout "crate/v$expected_version" "$test_root/missing-tag"

for invalid in main deadbeef 'crate/v1.2' 'refs/tags/crate/v1.2.3' '--upload-pack=other'; do
  expect_failure checkout "$invalid" "$test_root/invalid"
  [[ ! -e "$test_root/invalid" ]]
done
expect_failure checkout 0000000000000000000000000000000000000000 "$test_root/missing-sha"
expect_failure checkout "$newer" "$test_root/wrong-version"
grep -q 'JACS source version does not match the SDK dependency' "$test_root/last-failure.log"

mkdir "$test_root/existing"
expect_failure checkout "$reviewed" "$test_root/existing"
[[ ! -e "$test_root/existing/.git" ]]
printf 'keep me\n' > "$test_root/existing-file"
expect_failure checkout "$reviewed" "$test_root/existing-file"
[[ "$(< "$test_root/existing-file")" == 'keep me' ]]
ln -s "$test_root/absent" "$test_root/dangling"
expect_failure checkout "$reviewed" "$test_root/dangling"
[[ -L "$test_root/dangling" && ! -e "$test_root/absent" ]]
expect_failure checkout "$reviewed" "$test_root/by-sha"
[[ "$(git -C "$test_root/by-sha" rev-parse HEAD)" == "$reviewed" ]]

# A same-version bump must retain the reviewed source pin and do no builds.
mkdir -p "$test_root/bump/scripts" "$test_root/bump/rust/haiai" "$test_root/bump/.github/workflows"
cp "$root/scripts/bump-jacs-version.sh" "$test_root/bump/scripts/"
cp "$root/rust/haiai/Cargo.toml" "$test_root/bump/rust/haiai/"
cp "$root/.github/workflows/test.yml" "$test_root/bump/.github/workflows/"
bash "$test_root/bump/scripts/bump-jacs-version.sh" "$expected_version"
cmp "$root/.github/workflows/test.yml" "$test_root/bump/.github/workflows/test.yml"

# Exercise the actual version-changing script only on copied manifests. Lockfile
# tools are inert stubs, so this test never builds packages or accesses registries.
for file in rust/haiai-cli/Cargo.toml rust/hai-mcp/Cargo.toml python/pyproject.toml node/package.json node/publish.deps.json; do
  mkdir -p "$(dirname "$test_root/bump/$file")"
  cp "$root/$file" "$test_root/bump/$file"
done
mkdir "$test_root/tools"
for tool in cargo npm uv make; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$test_root/tools/$tool"
  chmod +x "$test_root/tools/$tool"
done
# The existing release script uses BSD sed. Normalize only its invocation in
# this Linux test fixture; changing the release script's portability is separate.
if sed --version >/dev/null 2>&1; then
  real_sed="$(command -v sed)"
  printf '#!/usr/bin/env bash\nif [[ "$1" == -i && "${2-unset}" == "" ]]; then shift 2; exec "%s" -i "$@"; fi\nexec "%s" "$@"\n' "$real_sed" "$real_sed" > "$test_root/tools/sed"
  chmod +x "$test_root/tools/sed"
fi

# A bad CI pin must fail before changing any manifest or the workflow itself.
printf 'env:\n  JACS_REF: main\n' > "$test_root/bump/.github/workflows/test.yml"
cp "$test_root/bump/.github/workflows/test.yml" "$test_root/invalid-pin.yml"
expect_failure env PATH="$test_root/tools:$PATH" bash "$test_root/bump/scripts/bump-jacs-version.sh" 98.98.98
grep -q 'Expected one exact JACS source pin in test.yml' "$test_root/last-failure.log"
for file in rust/haiai/Cargo.toml rust/haiai-cli/Cargo.toml rust/hai-mcp/Cargo.toml python/pyproject.toml node/package.json node/publish.deps.json; do
  cmp "$root/$file" "$test_root/bump/$file"
done
cmp "$test_root/invalid-pin.yml" "$test_root/bump/.github/workflows/test.yml"
cp "$root/.github/workflows/test.yml" "$test_root/bump/.github/workflows/test.yml"

for next_version in 98.98.98 98.98.99; do
  PATH="$test_root/tools:$PATH" bash "$test_root/bump/scripts/bump-jacs-version.sh" "$next_version" > "$test_root/bump.log"
  grep -Fxq "  JACS_REF: crate/v$next_version" "$test_root/bump/.github/workflows/test.yml"
done

echo 'JACS checkout checks passed: exact SHA/tag, detached HEAD, version, refusal, and safe bump transitions.'
