#!/usr/bin/env bash
# Tries the release signing scripts with a throw-away key: a good release verifies, and a changed installer,
# a changed checksum list and the wrong key each do not. Needs gpg and sha256sum; skips without them.
#
#   scripts/test-signing.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
command -v gpg >/dev/null && command -v sha256sum >/dev/null || { echo "skipping: no gpg or sha256sum"; exit 0; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
newkey() { # newkey HOME NAME
  mkdir -p "$1"; chmod 700 "$1"
  GNUPGHOME="$1" gpg --batch --quiet --pinentry-mode loopback --passphrase 'a test passphrase' --quick-gen-key "$2 <test@example.invalid>" ed25519 sign never 2>/dev/null
}
fails=0
expect() { # expect pass|fail DESCRIPTION COMMAND...
  local want="$1" what="$2"; shift 2
  if "$@" >"$work/out" 2>&1; then got=pass; else got=fail; fi
  if [ "$got" = "$want" ]; then echo "ok   $what"; else echo "FAIL $what (wanted $want)"; sed 's/^/     /' "$work/out"; fails=$((fails + 1)); fi
}

newkey "$work/signer" signer
newkey "$work/other" other
rel="$work/rel"; mkdir -p "$rel/deb" "$rel/win"
echo deb > "$rel/deb/SSHVault_1.0_amd64.deb"; echo rpm > "$rel/deb/SSHVault-1.0.rpm"; echo app > "$rel/deb/SSHVault_1.0.AppImage"
echo msi > "$rel/win/SSHVault_1.0.msi"; echo exe > "$rel/win/SSHVault_1.0_x64-setup.exe"
echo sig > "$rel/deb/SSHVault_1.0.AppImage.sig"   # an updater signature: not an installer, left alone
GNUPGHOME="$work/signer" gpg --armor --export > "$work/signer.pub"
GNUPGHOME="$work/other" gpg --armor --export > "$work/other.pub"

GPG_PASSPHRASE='a test passphrase' GNUPGHOME="$work/signer" "$here/sign-release.sh" "$rel" >/dev/null
[ ! -e "$rel/deb/SSHVault_1.0.AppImage.sig.asc" ] && echo "ok   the updater's own .sig is not signed again" || { echo "FAIL signed a .sig"; fails=$((fails + 1)); }
[ "$(wc -l < "$rel/SHA256SUMS")" -eq 5 ] && echo "ok   SHA256SUMS lists the five installers" || { echo "FAIL SHA256SUMS"; fails=$((fails + 1)); }

flat="$work/flat"; mkdir "$flat"; find "$rel" -type f -exec cp {} "$flat/" \;   # a release is a flat list of downloads
expect pass "a good release verifies" "$here/verify-release.sh" "$flat" "$work/signer.pub"
expect fail "the wrong key does not" "$here/verify-release.sh" "$flat" "$work/other.pub"
echo changed >> "$flat/SSHVault_1.0_amd64.deb"
expect fail "a changed installer does not" "$here/verify-release.sh" "$flat" "$work/signer.pub"
cp "$rel/deb/SSHVault_1.0_amd64.deb" "$flat/"
sed -i 's/^[0-9a-f]\{5\}/00000/' "$flat/SHA256SUMS"
expect fail "a changed checksum list does not" "$here/verify-release.sh" "$flat" "$work/signer.pub"
cp "$rel/SHA256SUMS" "$flat/"
rm "$flat/SSHVault_1.0.msi.asc"
expect fail "a missing signature does not" "$here/verify-release.sh" "$flat" "$work/signer.pub"
expect fail "signing with no installers is an error" env GNUPGHOME="$work/signer" "$here/sign-release.sh" "$work/flat/none"
mkdir "$work/empty"
expect fail "signing an empty folder is an error" env GNUPGHOME="$work/signer" "$here/sign-release.sh" "$work/empty"

[ "$fails" -eq 0 ] && echo "all signing checks passed" || { echo "$fails failed"; exit 1; }
