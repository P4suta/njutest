# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail
codeql_version=2.27.1
os_type="$(uname -s)"
architecture="$(uname -m)"
case "$os_type" in
  Darwin) platform=osx64; cache_root="${XDG_CACHE_HOME:-$HOME/Library/Caches}" ;;
  Linux)
    cache_root="${XDG_CACHE_HOME:-$HOME/.cache}"
    case "$architecture" in x86_64) platform=linux64 ;; aarch64|arm64) platform=linux-arm64 ;; *) echo "CodeQL has no bundle for $architecture" >&2; exit 2 ;; esac
    ;;
  MINGW*|MSYS*|CYGWIN*) platform=win64; cache_root="${XDG_CACHE_HOME:-$HOME/.cache}" ;;
  *) echo "CodeQL has no bundle for $os_type" >&2; exit 2 ;;
esac
codeql_root="${NJUTEST_CODEQL_CACHE:-$cache_root/njutest/codeql}/$codeql_version"
codeql_program="$codeql_root/codeql/codeql"
if [ "$platform" = win64 ]; then codeql_program="$codeql_root/codeql/codeql.exe"; fi
run_codeql() {
  if [ "$os_type" = Darwin ] && [ "$architecture" = arm64 ]; then
    arch -x86_64 /bin/bash "$codeql_program" "$@"
  else
    "$codeql_program" "$@"
  fi
}
case "${1:-check}" in
  setup)
    mkdir -p "$codeql_root"
    if [ ! -f "$codeql_program" ] || [ ! -d "$codeql_root/codeql/qlpacks/codeql/rust-queries" ] || [ ! -f "$codeql_root/codeql/rust/tools/index-files.sh" ]; then
      archive="codeql-bundle-$platform.tar.zst"
      gh release download "codeql-bundle-v$codeql_version" --repo github/codeql-action --pattern "$archive*" --dir "$codeql_root" --skip-existing
      (
        cd "$codeql_root"
        if command -v sha256sum >/dev/null; then
          sha256sum -c "$archive.checksum.txt"
        else
          shasum -a 256 -c "$archive.checksum.txt"
        fi
        tar -xf "$archive"
      )
    fi
    run_codeql version --format=json
    ;;
  check)
    if [ ! -f "$codeql_program" ]; then
      echo "security:codeql needs the pinned complete bundle; run mise run setup:codeql once before going offline" >&2
      exit 2
    fi
    run_codeql version --format=json | python3 -c 'import json,sys; version=json.load(sys.stdin)["version"]; assert version == "2.27.1", version'
    query_suites=("$codeql_root"/codeql/qlpacks/codeql/rust-queries/*/codeql-suites/rust-security-extended.qls)
    if [ "${#query_suites[@]}" -ne 1 ] || [ ! -f "${query_suites[0]}" ]; then
      echo "security:codeql needs the complete bundle's Rust query suite; run mise run setup:codeql" >&2
      exit 2
    fi
    mkdir -p target/codeql
    run_codeql database create target/codeql/database --overwrite --language=rust --build-mode=none --source-root=. --threads=6
    run_codeql database analyze target/codeql/database "${query_suites[0]}" --search-path="$codeql_root/codeql/qlpacks" --threads=6 --format=sarifv2.1.0 --output=target/codeql/results.sarif
    python3 - <<'PY'
import json
from pathlib import Path
document = json.loads(Path("target/codeql/results.sarif").read_text())
findings = sum(len(scan["results"]) for scan in document["runs"])
print(f"security:codeql: {findings} findings; SARIF: target/codeql/results.sarif")
raise SystemExit(1 if findings else 0)
PY
    ;;
  *) echo "expected setup or check" >&2; exit 2 ;;
esac
