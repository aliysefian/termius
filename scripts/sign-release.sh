#!/usr/bin/env bash
# Sign what a release ships, with a GPG key: a detached signature next to every installer, and a
# SHA256SUMS file (signed as SHA256SUMS.asc) that lists them all.
#
#   scripts/sign-release.sh DIR            sign the installers under DIR (searched recursively)
#
# Environment:
#   GPG_KEY_ID       the key to sign with (default: gpg's default secret key)
#   GPG_PASSPHRASE   its passphrase, if it has one (read without a terminal)
#   GNUPGHOME        where the key lives, as for gpg
#
# Installers: .deb .rpm .AppImage .msi and *-setup.exe. The Windows ones are also signed with
# Authenticode by the build; this signature is the second, independent way to check them.
# SHA256SUMS lists the files by bare name, so `sha256sum -c SHA256SUMS` works in the folder
# they are downloaded to. Verify with scripts/verify-release.sh.
set -euo pipefail

dir="${1:?usage: sign-release.sh DIR}"
[ -d "$dir" ] || { echo "not a folder: $dir" >&2; exit 2; }
command -v gpg >/dev/null || { echo "gpg is not installed" >&2; exit 2; }
command -v sha256sum >/dev/null || { echo "sha256sum is not installed" >&2; exit 2; }

sign() {
  local args=(--batch --yes --armor --detach-sign)
  [ -n "${GPG_KEY_ID:-}" ] && args+=(--local-user "$GPG_KEY_ID")
  if [ -n "${GPG_PASSPHRASE:-}" ]; then
    args+=(--pinentry-mode loopback --passphrase-fd 3)
    gpg "${args[@]}" --output "$1.asc" "$1" 3<<<"$GPG_PASSPHRASE"
  else
    gpg "${args[@]}" --output "$1.asc" "$1"
  fi
}

mapfile -t files < <(find "$dir" -type f \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' -o -name '*.msi' -o -name '*-setup.exe' \) | sort)
[ "${#files[@]}" -gt 0 ] || { echo "no installers under $dir" >&2; exit 1; }

# Two files with one name would make one line of SHA256SUMS ambiguous.
dupes=$(for f in "${files[@]}"; do basename "$f"; done | sort | uniq -d)
[ -z "$dupes" ] || { echo "the same file name twice: $dupes" >&2; exit 1; }

for f in "${files[@]}"; do
  sign "$f"
  echo "signed $(basename "$f")"
done

sums="$dir/SHA256SUMS"
: > "$sums"
for f in "${files[@]}"; do
  (cd "$(dirname "$f")" && sha256sum "$(basename "$f")") >> "$sums"
done
sign "$sums"
echo "wrote SHA256SUMS and SHA256SUMS.asc (${#files[@]} files)"
