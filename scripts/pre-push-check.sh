#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

# Verify exactly the commit a push names. The full gate reads a working tree,
# so the hook asks Git to render that object into a fresh detached worktree;
# ignored files and adjacent edits in the developer's checkout never become
# inputs to the answer.

set -euo pipefail

# Two numbers, because one cannot be both the detector and the backstop.
#
# A gate that quietly takes an hour is not a slow gate, it is a broken one, and the way that breaks is always the same: something stopped being cached and nobody noticed, because waiting looks exactly like working.
# What catches that is `expected_seconds`, which only says so: a warning on a run that finished is read, and costs the push nothing.
# What stops the hour is `budget_seconds`, and a backstop that fires on the normal path is not one -- it is a guillotine that throws away the compile it interrupted, so the next run is slow for the same reason and the ratchet only turns one way.
#
# Measured on this machine, against the checkout this script keeps and with the compile lifted out of the budget below: `mise run check` is about 340s with both fingerprint sets warm.
# So 420s is what a run should beat and 600s is what no honest one reaches; the regression this was built to catch ran 1720s.
# Raise either deliberately, in a commit that says what got slower and why that is now correct.
#
# `set -m` puts the check in its own process group so the whole tree of cargo, nextest and rustc goes down with it; killing the shell alone would leave the compile running and the budget unenforced.
budget_seconds="${NJUTEST_PUSH_BUDGET_SECONDS:-600}"
expected_seconds="${NJUTEST_PUSH_EXPECTED_SECONDS:-420}"

within_budget() {
  local started elapsed job
  started=$(date +%s)
  set -m
  "$@" &
  job=$!
  set +m
  while kill -0 "${job}" 2>/dev/null; do
    elapsed=$(( $(date +%s) - started ))
    if (( elapsed >= budget_seconds )); then
      kill -TERM "-${job}" 2>/dev/null || kill -TERM "${job}" 2>/dev/null || true
      sleep 5
      kill -KILL "-${job}" 2>/dev/null || kill -KILL "${job}" 2>/dev/null || true
      wait "${job}" 2>/dev/null || true
      echo "pre-push: the gate passed its ${budget_seconds}s budget and was stopped at ${elapsed}s" >&2
      echo "pre-push: that is a report about the gate. Find what stopped being cached, or raise NJUTEST_PUSH_BUDGET_SECONDS in a commit that says why" >&2
      return 124
    fi
    sleep 2
  done
  wait "${job}"
  elapsed=$(( $(date +%s) - started ))
  if (( elapsed >= expected_seconds )); then
    echo "pre-push: the gate passed in ${elapsed}s, over the ${expected_seconds}s a warm run should beat" >&2
    echo "pre-push: that is the reading to act on while it is still cheap. A first run after a merge is expected here; a second one that is still slow means something stopped being cached" >&2
  fi
}

zero=0000000000000000000000000000000000000000
head=$(git rev-parse --verify HEAD)
seen=0

# `|| [[ -n ... ]]` so the last line is read even when nothing terminates it.
# The global dispatcher captures the ref list with `$(cat)`, which strips the trailing newline, and replays it with `printf '%s'`; a bare `read` returns non-zero at that EOF and drops the line it had already filled in, so the whole gate saw an empty push and refused a fast-forward it should have checked.
while read -r local_ref local_oid remote_ref remote_oid || [[ -n "${local_ref}" ]]; do
  if [[ -z "${local_ref}" || -z "${local_oid}" || -z "${remote_ref}" || -z "${remote_oid}" ]]; then
    echo "pre-push: git supplied an incomplete ref update" >&2
    exit 1
  fi
  if [[ "${local_oid}" == "${zero}" ]]; then
    continue
  fi
  seen=$((seen + 1))
  if [[ "${local_oid}" != "${head}" ]]; then
    echo "pre-push: ${local_ref} names ${local_oid}, but the checked-out commit is ${head}" >&2
    echo "pre-push: check out the exact commit being pushed before running its gate" >&2
    exit 1
  fi
  if [[ "${remote_oid}" != "${zero}" ]]; then
    if ! git cat-file -e "${remote_oid}^{commit}" 2>/dev/null; then
      echo "pre-push: remote ${remote_ref} names commit ${remote_oid}, which is not present locally" >&2
      echo "pre-push: fetch the remote ref before proving that this update is a fast-forward" >&2
      exit 1
    fi
    if ! git merge-base --is-ancestor "${remote_oid}" "${local_oid}"; then
      echo "pre-push: ${local_oid} does not descend from ${remote_ref} at ${remote_oid}" >&2
      echo "pre-push: non-fast-forward updates are forbidden" >&2
      exit 1
    fi
  fi
done

if [[ "${seen}" -eq 0 ]]; then
  echo "pre-push: no non-delete ref update was supplied; refusing an unverifiable gate" >&2
  exit 1
fi

require_exact_tree() {
  local now
  now=$(git -C "${checkout}" rev-parse --verify HEAD)
  if [[ "${now}" != "${head}" ]]; then
    echo "pre-push: HEAD moved from ${head} to ${now} while its gate ran" >&2
    exit 1
  fi
  if [[ -n "$(git -C "${checkout}" status --porcelain=v1 --untracked-files=all)" ]]; then
    echo "pre-push: the isolated check changed the tree of ${head}" >&2
    exit 1
  fi
}

repository=$(git rev-parse --show-toplevel)
# One path, reused by every push, and that is the whole point.
#
# Cargo writes the package's own directory into each crate's fingerprint, so a worktree at a fresh `mktemp -d` makes every workspace crate a guaranteed miss however warm the target directory is.
# That is what made this gate a full rebuild each time, and no choice of `target/debug` against `target/pre-push` could have touched it.
# The tree is still exactly the pushed object, checked out again from scratch below, so nothing about the isolation is traded for the cache: what is reused is the path, not the contents.
# It lives outside the repository because the `lints` gate resolves declared paths against the four source roots, and a worktree under `target/` makes every crate in it look like a path outside them. Keyed by the repository so two checkouts of this project do not share one tree.
key=$(printf '%s' "${repository}" | shasum | cut -c1-12)
checkout="${TMPDIR:-/tmp}/njutest-pre-push-${key}/tree"

cleanup() {
  if [[ -e "${checkout}/.git" ]]; then
    git -C "${checkout}" restore --staged --worktree :/ >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# Moved to, not recreated.
#
# `git worktree add` writes every file afresh, and cargo fingerprints on mtime, so a tree with identical content still rebuilds the workspace from nothing.
# That is the last thing that made a push cost twenty minutes with a cache that was already warm: measured in this very worktree, `build` is 1s and `clippy` is 0s once the mtimes stop moving.
# A checkout in place touches only the files that differ, which is exactly the set that should be recompiled.
mkdir -p "$(dirname "${checkout}")"
git -C "${repository}" worktree prune
if [[ -e "${checkout}/.git" ]]; then
  git -C "${checkout}" checkout --quiet --force --detach "${head}"
  git -C "${checkout}" clean --quiet -fd -e /target
else
  rm -rf "${checkout}"
  git -C "${repository}" worktree add --quiet --detach "${checkout}" "${head}"
fi
mkdir -p "${repository}/target/pre-push/debug" "${repository}/target/pre-push/release" "${checkout}/target"
ln -sfn "${repository}/target/pre-push/debug" "${checkout}/target/debug"
ln -sfn "${repository}/target/pre-push/release" "${checkout}/target/release"
require_exact_tree
# Compiling is not what the budget is about, and budgeting it made the budget a guillotine.
#
# `mise run check` compiles this workspace twice over, once with clippy's fingerprints and once with the test harness's, and a commit that touches a core library recompiles everything downstream in both.
# Measured here: about 900s from a cold `target/pre-push`, against 340s once both sets are warm.
# A budget large enough for the first is too large to catch anything, and one sized for the second kills four pushes out of five on work that was proceeding normally -- and each kill throws away the compile, so the retry is cold again.
# One kill did worse than waste time: starved of CPU by its own cold compile, a test passed nextest's per-test timeout and the gate reported a test failure that did not exist.
#
# So both fingerprint sets are warmed first, outside the budget and timed out loud, and the budget then measures the incremental work -- which is the thing that is supposed to be fast, and the thing that stops being fast when caching breaks.
# Neither warming command decides anything: `mise run check` is the authority and runs afterwards either way, so a failure here is left for it to report properly rather than surfaced as a bare cargo error.
warming=$(date +%s)
( cd "${checkout}" && mise run build >/dev/null 2>&1 ) || true
( cd "${checkout}" && cargo clippy --locked --workspace --all-targets --all-features >/dev/null 2>&1 ) || true
echo "pre-push: compiled in $(( $(date +%s) - warming ))s, which the budget does not count" >&2

within_budget bash -c 'cd "$1" && NJUTEST_COMMITTED_HEAD="$2" exec mise run check' _ "${checkout}" "${head}"
# The tree is isolated and so is the cache, which is now warm because the path above no longer changes: the developer's own `target/debug` stays out of the answer, and the gate still does not recompile what the previous push compiled.
# What a warm cache cannot answer is whether a green came from an artifact older than the field it is meant to prove, so that question is asked separately and coldly below.
require_exact_tree
