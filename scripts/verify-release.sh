#!/usr/bin/env bash
# Check a downloaded release against the project's public key.
#
#   scripts/verify-release.sh DIR [PUBLIC_KEY.asc]
#
# DIR holds the downloaded installers with their .asc files and SHA256SUMS(.asc). The key is the
# one published with the release (sshvault-release.asc), or any file you got from a place you trust.
# The key is imported into a throw-away keyring, never into yours. Prints the key's fingerprint:
# compare it with the one in docs/SIGNING.md before you rely on the answer.
set -euo pipefail

dir="${1:?usage: verify-release.sh DIR [PUBLIC_KEY.asc]}"
key="${2:-$dir/sshvault-release.asc}"
[ -f "$key" ] || { echo "no public key at $key" >&2; exit 2; }
[ -f "$dir/SHA256SUMS" ] && [ -f "$dir/SHA256SUMS.asc" ] || { echo "SHA256SUMS or SHA256SUMS.asc is missing in $dir" >&2; exit 1; }

home="$(mktemp -d)"
trap 'rm -rf "$home"' EXIT
export GNUPGHOME="$home"
chmod 700 "$home"
gpg --batch --quiet --import "$key" 2>/dev/null
echo "Key fingerprint: $(gpg --batch --with-colons --fingerprint 2>/dev/null | awk -F: '/^fpr/ {print $10; exit}')"

fail=0
check() {
  if gpg --batch --verify "$1.asc" "$1" >/dev/null 2>&1; then echo "good signature: $(basename "$1")"; else echo "BAD or missing signature: $(basename "$1")" >&2; fail=1; fi
}
check "$dir/SHA256SUMS"
(cd "$dir" && sha256sum --quiet -c SHA256SUMS) && echo "checksums match" || { echo "CHECKSUM MISMATCH" >&2; fail=1; }
while read -r _ name; do
  name="${name#\*}"
  check "$dir/$name"
done < "$dir/SHA256SUMS"
[ "$fail" -eq 0 ] && echo "All good." || { echo "Do not install these files." >&2; exit 1; }
