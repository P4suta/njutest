#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

# Verify exactly the commit a push names. The full gate reads a working tree,
# so the hook asks Git to render that object into a fresh detached worktree;
# ignored files and adjacent edits in the developer's checkout never become
# inputs to the answer.

set -euo pipefail

zero=0000000000000000000000000000000000000000
head=$(git rev-parse --verify HEAD)
seen=0

while read -r local_ref local_oid remote_ref remote_oid; do
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
temporary=$(mktemp -d "${TMPDIR:-/tmp}/njutest-pre-push.XXXXXX")
checkout=${temporary}/tree

cleanup() {
  if [[ -e "${checkout}/.git" ]]; then
    git -C "${checkout}" restore --staged --worktree :/ >/dev/null 2>&1 || true
    if ! git -C "${repository}" worktree remove "${checkout}" >/dev/null; then
      echo "pre-push: could not remove temporary worktree ${checkout}" >&2
      return
    fi
  fi
  rmdir "${temporary}" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

git -C "${repository}" worktree add --quiet --detach "${checkout}" "${head}"
mkdir -p "${repository}/target/pre-push/debug" "${repository}/target/pre-push/release"
mkdir "${checkout}/target"
ln -s "${repository}/target/pre-push/debug" "${checkout}/target/debug"
ln -s "${repository}/target/pre-push/release" "${checkout}/target/release"
require_exact_tree
(cd "${checkout}" && NJUTEST_COMMITTED_HEAD="${head}" mise run check)
require_exact_tree
