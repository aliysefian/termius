# Signing and verifying releases

**Where things stand.** The machinery for signing Windows and Linux releases is in place and
switches on by itself when the keys are added to the GitHub repository. **No release has been signed
yet**, because no certificate or key has been added. Until then, installers are unsigned, Windows
SmartScreen warns on first launch, and the release notes say so.

This page is for two people: whoever publishes releases (how to turn signing on), and whoever
downloads one (how to check it).

## What gets signed

| What | How | Checked by |
|---|---|---|
| Windows `.msi` and `-setup.exe` | Authenticode with your code-signing certificate, timestamped; Tauri signs the programs inside and the installers | Windows itself (SmartScreen, file properties), `Get-AuthenticodeSignature`; the build fails if an installer isn't validly signed |
| Every installer (`.deb`, `.rpm`, `.AppImage`, `.msi`, `-setup.exe`) | A detached GPG signature, `<file>.asc` | `scripts/verify-release.sh` or `gpg --verify` |
| The list of checksums | `SHA256SUMS`, signed as `SHA256SUMS.asc` | `scripts/verify-release.sh` or `sha256sum -c` |
| Updates (`.sig`, `latest.json`) | The app's own update key (Tauri), a separate system; see "Updates" in `DEVELOPMENT.md` | The app, before installing an update |

The Windows installers have both signatures on purpose: Authenticode is what Windows trusts, the GPG
one is a second, independent way to check the same bytes.

## Checking a download

**Windows.** Right-click the installer → Properties → Digital Signatures, or in PowerShell:

```powershell
Get-AuthenticodeSignature .\SSHVault_*_x64-setup.exe | Format-List Status, SignerCertificate
```

`Status` must be `Valid` and the signer must be who you expect.

**Linux (and Windows installers too).** Put every file of the release in one folder (the installers,
their `.asc` files, `SHA256SUMS`, `SHA256SUMS.asc` and `sshvault-release.asc`), then run, from a copy of this
repository:

```bash
scripts/verify-release.sh /path/to/folder
```

It imports the key into a throw-away keyring (never into yours), prints the key's fingerprint, checks the
signature on `SHA256SUMS`, checks every file against it, and checks every file's own signature. It prints
`All good.` or says what failed and exits with an error. **Compare the fingerprint with the one below
or in the release notes**; the key file comes from the same place as the installer, so a matching
fingerprint from a second place is what makes the check mean something.

By hand:

```bash
gpg --import sshvault-release.asc
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
gpg --verify SSHVault_1.2.3_amd64.deb.asc SSHVault_1.2.3_amd64.deb
```

**Release key fingerprint:** *not created yet.* It will be written here when the first signed release is made.

The `.deb` and `.rpm` files carry no signature inside them (a detached signature is not what `dpkg` or
`rpm -K` look at), and there is no apt or yum repository yet, so `apt` and `dnf` do not check anything
by themselves. Verify first, then install.

## Turning signing on (for whoever publishes)

Add these in the repository's **Settings → Secrets and variables → Actions**. Any one can be added
alone: each switches on its own part.

### Windows

You need a code-signing certificate from a certificate authority. An *OV* certificate signs
installers; an *EV* certificate (or Microsoft's Trusted Signing service) also builds SmartScreen
reputation faster, which is what stops the warning. Whichever you buy, export it as a `.pfx`:

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | The `.pfx`, base64-encoded (`[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`) |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |

The workflow imports it, signs through Tauri, timestamps with DigiCert's server, and then **verifies every
installer**; an unsigned or untrusted one fails the build. (Certificates kept on a hardware token or in a
cloud HSM, as most EV ones now are, can't be exported as a file. They need a different step, such as the
signing service's own action; that is not wired here.)

### Linux

Make a key just for releases, with a passphrase:

```bash
gpg --quick-generate-key "SSHVault releases <releases@example.org>" ed25519 sign 2y
gpg --list-secret-keys --keyid-format long          # note the fingerprint
gpg --armor --export-secret-keys <fingerprint>      # the private key, armored
gpg --armor --export <fingerprint> > sshvault-release.asc
```

| Secret | Value |
|---|---|
| `GPG_PRIVATE_KEY` | The armored private key (the output of `--export-secret-keys`, as text) |
| `GPG_PASSPHRASE` | Its passphrase |

Then write the fingerprint into this page and into the project's README, from a place other than the
release itself. Keep an offline copy of the private key and its revocation certificate
(`gpg --gen-revoke`). The key on GitHub can sign; it cannot be taken back out, so use a key you
can afford to revoke (a signing subkey, with the main key kept offline, is better still).

The release job imports the key, signs everything, **verifies the result the way a downloader would**
(a release that does not verify is not published), and attaches `*.asc`, `SHA256SUMS`, `SHA256SUMS.asc`
and `sshvault-release.asc` to the release.

### Rotating

Make a new key, add its secrets, publish the new fingerprint, and say so in the release notes. Sign
`SHA256SUMS` of the first release made with the new key with the old key too, if you still have it, so there
is a chain from one to the next.

## What is tested, and what is not

`scripts/test-signing.sh` (it runs with the other checks on every push) makes a throw-away key and
checks that a good release verifies, and that a changed installer, a changed checksum list, a missing
signature and the wrong key each do not.

**Not tested here:** a real code-signing certificate and the Windows verification step (no certificate
was available), the workflow's secret handling and import of a real key on a GitHub runner, and the
release notes' wording for the signed case. They run the first time a release is made with the secrets
set, so check that release's Actions log before announcing it.

## Not done

- An apt and a yum repository with the key published, so package managers verify on their own.
- A signature inside the `.rpm` (`rpmsign`) and the `.deb`.
- macOS (see `feat2.md`): code signing and notarisation come with the macOS build.
- Certificates on hardware tokens or cloud HSMs (see above).
- Reproducible builds, so anyone can rebuild an installer and compare it with the signed one.
