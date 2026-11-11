#!/usr/bin/env bash
# Tries scripts/ci-apt.sh without apt, root or network: the mirror ranking with scripted speeds, the rewrite on a copy of apt's
# files, and the order of the tries with a command that fails a set number of times.
#
#   scripts/test-ci-apt.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export SUDO="" APT_DIR="$work/apt"
fail() { echo "FAIL: $*" >&2; exit 1; }

fresh() {
  rm -rf "$APT_DIR"; mkdir -p "$APT_DIR/sources.list.d"
  printf '%s\n' 'deb mirror+file:/etc/apt/apt-mirrors.txt jammy main restricted' 'deb http://security.ubuntu.com/ubuntu jammy-security main' 'deb https://archive.ubuntu.com/ubuntu/ jammy universe' > "$APT_DIR/sources.list"
  printf '%s\n' 'http://azure.archive.ubuntu.com/ubuntu/	priority:1' 'https://archive.ubuntu.com/ubuntu/	priority:2' > "$APT_DIR/apt-mirrors.txt"
  printf '%s\n' 'URIs: https://example.com/ubuntu/' > "$APT_DIR/sources.list.d/other.sources"
}

fresh
"$here/ci-apt.sh" setup
grep -q 'Acquire::http::Timeout "20"' "$APT_DIR/apt.conf.d/99ci-network" || fail "no network timeout written"
grep -q 'Acquire::Retries' "$APT_DIR/apt.conf.d/99ci-network" || fail "no retries written"
"$here/ci-apt.sh" bogus 2>/dev/null && fail "unknown command accepted"

# shellcheck source=scripts/ci-apt.sh
source "$here/ci-apt.sh"
set +e
# Scripted speeds: the runner's own mirror is the slowest, one does not answer.
probe() {
  case "$1" in
    azure.archive.ubuntu.com) echo 21000 ;; archive.ubuntu.com) echo 135000 ;; mirrors.edge.kernel.org) echo 494000 ;;
    mirrors.ocf.berkeley.edu) echo 3558000 ;; mirror.us.leaseweb.net) echo 0 ;; mirrors.mit.edu) echo 410000 ;;
  esac
}
rank_mirrors > /dev/null
[ "${MIRRORS[*]}" = "mirrors.ocf.berkeley.edu mirrors.edge.kernel.org mirrors.mit.edu archive.ubuntu.com" ] || fail "ranking: ${MIRRORS[*]}"

# Fails twice, then works: three tries, on the three fastest mirrors in order.
count="$work/count"; echo 0 > "$count"; : > "$work/seen"
flaky() { local n; n=$(<"$count"); echo $((n + 1)) > "$count"; grep -o 'http://[a-z.]*' "$APT_DIR/apt-mirrors.txt" | head -n 1 >> "$work/seen"; [ "$n" -ge 2 ]; }
retry flaky > "$work/out" 2>&1 || { cat "$work/out"; fail "retry did not recover"; }
[ "$(<"$count")" = 3 ] || fail "expected 3 tries, saw $(<"$count")"
[ "$(tr '\n' ' ' < "$work/seen")" = "http://mirrors.ocf.berkeley.edu http://mirrors.edge.kernel.org http://mirrors.mit.edu " ] || fail "tries used: $(tr '\n' ' ' < "$work/seen")"
! grep -q 'azure.archive' "$APT_DIR/apt-mirrors.txt" || fail "the slow mirror was left in"
grep -q 'https://mirrors.mit.edu/ubuntu/ jammy universe' "$APT_DIR/sources.list" || fail "sources.list not rewritten: $(cat "$APT_DIR/sources.list")"
grep -q 'security.ubuntu.com' "$APT_DIR/sources.list" || fail "the security mirror must not be rewritten"
grep -q 'example.com/ubuntu' "$APT_DIR/sources.list.d/other.sources" || fail "a third-party source was rewritten"
grep -q 'mirror+file:/etc/apt/apt-mirrors.txt' "$APT_DIR/sources.list" || fail "the mirror+file line was damaged"

# Always fails: stops after the four ranked mirrors, and says so.
fresh; echo 0 > "$count"
always() { local n; n=$(<"$count"); echo $((n + 1)) > "$count"; return 1; }
if retry always > "$work/out" 2>&1; then fail "a command that always fails succeeded"; fi
[ "$(<"$count")" = 4 ] || fail "expected 4 tries, saw $(<"$count")"
grep -q '::error::apt did not finish after 4 tries' "$work/out" || fail "no error annotation"

# Nothing measurable: the list is used as written, so there is still something to try.
probe() { echo 0; }
rank_mirrors > /dev/null
[ "${MIRRORS[*]}" = "azure.archive.ubuntu.com archive.ubuntu.com mirrors.edge.kernel.org mirrors.ocf.berkeley.edu" ] || fail "fallback: ${MIRRORS[*]}"
echo "ci-apt: ok"
