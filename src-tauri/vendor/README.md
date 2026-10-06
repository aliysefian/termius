# Vendored crates

## ironrdp-session 0.11.0

A copy of the published crate with one fix in `src/fast_path.rs`
(`crop_rows`, and its three call sites in `process_bitmap_update`).

Servers such as xrdp pad each bitmap row to a multiple of four pixels, so the
bitmap is wider than the rectangle it paints. The published crate reads the
rows as if they were the rectangle's width, which shears the picture into
diagonal stripes. The patch keeps only the rectangle's columns.

Drop this folder and the `[patch.crates-io]` entry in `../Cargo.toml` when a
release of `ironrdp-session` fixes it (0.11.0 was the latest at the time).

## picky 7.0.0-rc.25 and sspi 0.21.3

Copies of the published crates with only their `Cargo.toml` changed.

`ironrdp-connector` pins `picky` to exactly `7.0.0-rc.25`, and `picky` pins a
whole set of RustCrypto *release candidates* (`curve25519-dalek =5.0.0-rc.1`,
`ed25519-dalek =3.0.0-rc.1`, ...). `russh`, which the SSH engine uses, needs the
final releases of the same crates, and Cargo can't hold both in one build.
`sspi` has the same pins for macOS and iOS. The copies ask for the final
releases (and keep the release candidates for `rsa`, `pkcs1` and the
`rustcrypto-ff` family, which have no final yet) so one set is chosen.

No source was changed; the code builds and runs against the final releases
(see the live RDP test). The macOS-only part of `sspi` hasn't been built, and
macOS builds aren't published.

`Cargo.lock` also holds `picky-krb` at 0.12.4: 0.12.5 adds an enum variant that
this `sspi` doesn't handle.

Drop these folders and their `[patch.crates-io]` entries when a release of
`ironrdp-connector` / `sspi` no longer pins release candidates.
