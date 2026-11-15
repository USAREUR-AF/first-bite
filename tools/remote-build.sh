#!/usr/bin/env bash
# Copy this working tree to a build host and run a command there.
#
#   REMOTE_BUILD_HOST=<ssh host> tools/remote-build.sh cargo test --workspace
#
# Each branch gets its own directory on the host, so its target/ is kept
# between runs. Cargo.lock is copied back after the command.
set -euo pipefail
host="${REMOTE_BUILD_HOST:?set REMOTE_BUILD_HOST to an ssh host}"
root="$(git rev-parse --show-toplevel)"
branch="$(git -C "$root" rev-parse --abbrev-ref HEAD | tr '/' '-')"
dir="${REMOTE_BUILD_DIR:-fb-build}/${REMOTE_BUILD_NAME:-$branch}"
# shellcheck disable=SC2029  # the directory name is meant to expand here
ssh "$host" "mkdir -p '$dir'"
rsync -az --delete \
  --exclude .git --exclude target --exclude vendor --exclude .claude/worktrees \
  --exclude private --exclude .leak-needles \
  --exclude '*.pyc' --exclude __pycache__ \
  "$root/" "$host:$dir/"
rc=0
# shellcheck disable=SC2029
ssh "$host" "cd '$dir' && export PATH=\$HOME/.cargo/bin:\$HOME/.local/bin:\$PATH && $*" || rc=$?
rsync -az "$host:$dir/Cargo.lock" "$root/Cargo.lock" 2>/dev/null || true
exit $rc
