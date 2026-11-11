#!/usr/bin/env bash
# apt on a Linux CI runner that cannot be stalled by one bad mirror.
#
# The runners' mirrors (azure.archive.ubuntu.com, then archive.ubuntu.com) sometimes accept the connection and never answer.
# apt waits without a limit, and the job runs out its whole timeout: the v0.60.0 release lost 45 minutes that way, twice.
# This gives apt short network timeouts and retries, bounds every attempt, and moves to another mirror after a failed one.
#
#   scripts/ci-apt.sh setup              only the timeouts and retries (for tools that run apt themselves)
#   scripts/ci-apt.sh install PKG...     apt-get update, then install the packages
#   scripts/ci-apt.sh run CMD...         the timeouts, then CMD, repeated like install (e.g. playwright install --with-deps)
#
# SUDO and APT_DIR can be overridden, which is how scripts/test-ci-apt.sh tries it on a copy of apt's files.
set -uo pipefail

SUDO="${SUDO-sudo}"
APT_DIR="${APT_DIR:-/etc/apt}"
# Tried in this order after the runner's own choice has failed; the last one is the runner's usual first choice again.
MIRRORS=(archive.ubuntu.com mirrors.edge.kernel.org azure.archive.ubuntu.com)

setup() {
  $SUDO mkdir -p "$APT_DIR/apt.conf.d"
  $SUDO tee "$APT_DIR/apt.conf.d/99ci-network" >/dev/null <<'CONF'
Acquire::Retries "3";
Acquire::http::Timeout "20";
Acquire::https::Timeout "20";
Acquire::ForceIPv4 "true";
CONF
}

# Point every Ubuntu archive entry at one mirror. Covers sources.list(.d) and the runner image's apt-mirrors.txt
# (its "mirror+file:" lines list the hosts there). security.ubuntu.com is left alone.
switch_mirror() {
  local m=$1 f
  echo "::warning::apt: trying the mirror $m"
  for f in "$APT_DIR"/sources.list "$APT_DIR"/sources.list.d/* "$APT_DIR"/apt-mirrors.txt; do
    [ -f "$f" ] || continue
    $SUDO sed -i -E "s#(https?://)([a-z0-9.-]*archive\.ubuntu\.com|mirrors\.edge\.kernel\.org)/ubuntu/?#\1$m/ubuntu/#g" "$f"
  done
}

# Run "$@" up to four times: the runner's own mirror first, then the others. Each try is cut off after its own limit
# by the command itself (see do_install, and the timeout in `run`).
retry() {
  local tries=$(( ${#MIRRORS[@]} + 1 )) n=1 m
  while :; do
    echo "apt: try $n of $tries"
    $SUDO dpkg --configure -a >/dev/null 2>&1 || true
    if "$@"; then return 0; fi
    echo "::warning::apt: try $n failed or stalled"
    [ "$n" -ge "$tries" ] && break
    m=${MIRRORS[$((n - 1))]}
    switch_mirror "$m"
    n=$((n + 1))
  done
  echo "::error::apt did not finish after $tries tries"
  return 1
}

do_install() {
  $SUDO timeout -k 30 600 apt-get update && $SUDO timeout -k 30 900 apt-get install -y --no-install-recommends "$@"
}

case "${1:-}" in
  setup) setup ;;
  install) shift; setup; retry do_install "$@" ;;
  run) shift; setup; retry timeout -k 30 900 "$@" ;;
  *) echo "usage: $0 setup | install PKG... | run CMD..." >&2; exit 2 ;;
esac
