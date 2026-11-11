#!/usr/bin/env bash
# apt on a Linux CI runner that a slow or silent Ubuntu mirror cannot stall.
#
# The runners' mirrors (azure.archive.ubuntu.com, then archive.ubuntu.com) are sometimes very slow, or accept the connection and
# never answer. apt has no limit of its own, so the job ran out its whole timeout: the v0.60.0 release lost 45 minutes that
# way, twice. A step that takes 30 seconds on a good day must not be able to do that. So this:
#   * measures the known mirrors first (a few seconds, all at once) and uses the fastest;
#   * gives apt short network timeouts and retries;
#   * cuts every try off after a few minutes and moves to the next-fastest mirror. What was already downloaded is kept.
# Every mirror serves the same signed archive: apt checks the signatures with Ubuntu's keys whichever one is used.
#
#   scripts/ci-apt.sh setup              only the timeouts and retries (for tools that run apt themselves)
#   scripts/ci-apt.sh install PKG...     apt-get update, then install the packages
#   scripts/ci-apt.sh run CMD...         the same tries around CMD (e.g. playwright install --with-deps)
#
# SUDO and APT_DIR can be overridden, and the file can be sourced: that is how scripts/test-ci-apt.sh tries it without apt.
set -uo pipefail

SUDO="${SUDO-sudo}"
APT_DIR="${APT_DIR:-/etc/apt}"
# Everything that may be used. All keep the archive under /ubuntu.
CANDIDATES=(azure.archive.ubuntu.com archive.ubuntu.com mirrors.edge.kernel.org mirrors.ocf.berkeley.edu mirror.us.leaseweb.net mirrors.mit.edu)
MAX_TRIES=4
# The mirrors to use, fastest first (filled by rank_mirrors).
MIRRORS=()
CODENAME="$( (. /etc/os-release 2>/dev/null && echo "${VERSION_CODENAME:-}") || true)"
CODENAME="${CODENAME:-jammy}"

setup() {
  $SUDO mkdir -p "$APT_DIR/apt.conf.d"
  $SUDO tee "$APT_DIR/apt.conf.d/99ci-network" >/dev/null <<'CONF'
Acquire::Retries "3";
Acquire::http::Timeout "20";
Acquire::https::Timeout "20";
Acquire::ForceIPv4 "true";
CONF
}

# Bytes per second a mirror gave for the release's index, measured for at most 8 seconds; 0 when it did not answer properly.
probe() {
  local s rc
  s=$(curl -sf -o /dev/null -m 8 -w '%{speed_download}' "http://$1/ubuntu/dists/$CODENAME/InRelease" 2>/dev/null)
  rc=$?
  # 28 is "ran out of time": slow, but what it managed is still its speed.
  if [ "$rc" -eq 0 ] || [ "$rc" -eq 28 ]; then echo "${s%%.*}"; else echo 0; fi
}

# Fill MIRRORS with the candidates that answered, fastest first. With none measured (no curl, no network yet) the list is
# used as written.
rank_mirrors() {
  local dir m
  dir=$(mktemp -d)
  for m in "${CANDIDATES[@]}"; do (probe "$m" > "$dir/$m") & done
  wait
  mapfile -t MIRRORS < <(for m in "${CANDIDATES[@]}"; do echo "$(cat "$dir/$m" 2>/dev/null || echo 0) $m"; done | sort -rn | awk '$1 > 0 { print $2 }' | head -n "$MAX_TRIES")
  echo "apt: mirror speeds (bytes/s): $(for m in "${CANDIDATES[@]}"; do printf '%s=%s ' "$m" "$(cat "$dir/$m" 2>/dev/null || echo 0)"; done)"
  rm -rf "$dir"
  [ "${#MIRRORS[@]}" -gt 0 ] || MIRRORS=("${CANDIDATES[@]:0:$MAX_TRIES}")
}

# Point every Ubuntu archive entry at one mirror. Covers sources.list(.d) and the runner image's apt-mirrors.txt
# (its "mirror+file:" lines list the hosts there). security.ubuntu.com and anything that is not a known mirror are left alone.
switch_mirror() {
  local m=$1 f hosts
  hosts=$(printf '%s|' "${CANDIDATES[@]}" | sed -e 's/\./\\./g' -e 's/|$//')
  echo "apt: using the mirror $m"
  for f in "$APT_DIR"/sources.list "$APT_DIR"/sources.list.d/* "$APT_DIR"/apt-mirrors.txt; do
    [ -f "$f" ] || continue
    $SUDO sed -i -E "s#(https?://)([a-z0-9.-]*archive\.ubuntu\.com|$hosts)/ubuntu/?#\1$m/ubuntu/#g" "$f"
  done
}

# Run "$@" once per mirror, fastest first, until it works. Each try is cut off by the command itself (see do_install and `run`).
retry() {
  local n=0 m
  rank_mirrors
  for m in "${MIRRORS[@]}"; do
    n=$((n + 1))
    echo "apt: try $n of ${#MIRRORS[@]}"
    switch_mirror "$m"
    $SUDO dpkg --configure -a >/dev/null 2>&1 || true
    if "$@"; then return 0; fi
    echo "::warning::apt: try $n ($m) failed or was too slow"
  done
  echo "::error::apt did not finish after $n tries"
  return 1
}

do_install() {
  $SUDO timeout -k 20 90 apt-get update && $SUDO timeout -k 20 360 apt-get install -y --no-install-recommends "$@"
}

# Only when run, not when sourced by the test.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  case "${1:-}" in
    setup) setup ;;
    install) shift; setup; retry do_install "$@" ;;
    run) shift; setup; retry timeout -k 20 420 "$@" ;;
    *) echo "usage: $0 setup | install PKG... | run CMD..." >&2; exit 2 ;;
  esac
fi
