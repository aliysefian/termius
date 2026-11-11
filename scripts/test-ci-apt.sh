#!/usr/bin/env bash
# Tries scripts/ci-apt.sh without apt or root: the mirror rewrite on a copy of apt's files, and the retry order with a command that
# fails a set number of times.
#
#   scripts/test-ci-apt.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export SUDO="" APT_DIR="$work/apt"
mkdir -p "$APT_DIR/sources.list.d"
fail() { echo "FAIL: $*" >&2; exit 1; }

cat > "$APT_DIR/sources.list" <<'S'
deb mirror+file:/etc/apt/apt-mirrors.txt jammy main restricted
deb http://security.ubuntu.com/ubuntu jammy-security main
deb https://archive.ubuntu.com/ubuntu/ jammy universe
S
cat > "$APT_DIR/apt-mirrors.txt" <<'S'
http://azure.archive.ubuntu.com/ubuntu/	priority:1
https://archive.ubuntu.com/ubuntu/	priority:2
S
cat > "$APT_DIR/sources.list.d/other.sources" <<'S'
URIs: https://example.com/apt
S

"$here/ci-apt.sh" setup
grep -q 'Acquire::http::Timeout "20"' "$APT_DIR/apt.conf.d/99ci-network" || fail "no network timeout written"
grep -q 'Acquire::Retries' "$APT_DIR/apt.conf.d/99ci-network" || fail "no retries written"

# retry: fails twice, then works; each failure moves to the next mirror.
count="$work/count"; echo 0 > "$count"
flaky() { local n; n=$(<"$count"); echo $((n + 1)) > "$count"; [ "$n" -ge 2 ]; }
export -f flaky; export count
sed -n '/^switch_mirror()/,/^do_install()/p' "$here/ci-apt.sh" | sed '$d' > "$work/fn.sh"
( MIRRORS=(archive.ubuntu.com mirrors.edge.kernel.org azure.archive.ubuntu.com); source "$work/fn.sh"; retry flaky ) > "$work/out" 2>&1 || { cat "$work/out"; fail "retry did not recover"; }
[ "$(<"$count")" = 3 ] || fail "expected 3 tries, saw $(<"$count")"
grep -q 'mirrors.edge.kernel.org/ubuntu/' "$APT_DIR/apt-mirrors.txt" || fail "second failure did not move to the second mirror: $(cat "$APT_DIR/apt-mirrors.txt")"
! grep -q 'azure.archive' "$APT_DIR/apt-mirrors.txt" || fail "old mirror left behind"
grep -q 'security.ubuntu.com' "$APT_DIR/sources.list" || fail "security mirror must not be rewritten"
grep -q 'example.com/apt' "$APT_DIR/sources.list.d/other.sources" || fail "a third-party source was rewritten"
grep -q 'mirror+file:/etc/apt/apt-mirrors.txt' "$APT_DIR/sources.list" || fail "the mirror+file line was damaged"

# retry: gives up after every mirror was tried, and says so.
echo 0 > "$count"
always() { local n; n=$(<"$count"); echo $((n + 1)) > "$count"; return 1; }
export -f always
if ( MIRRORS=(archive.ubuntu.com mirrors.edge.kernel.org azure.archive.ubuntu.com); source "$work/fn.sh"; retry always ) > "$work/out" 2>&1; then fail "a command that always fails succeeded"; fi
[ "$(<"$count")" = 4 ] || fail "expected 4 tries, saw $(<"$count")"
grep -q '::error::apt did not finish after 4 tries' "$work/out" || fail "no error annotation"

# usage
"$here/ci-apt.sh" bogus 2>/dev/null && fail "unknown command accepted"
echo "ci-apt: ok"
