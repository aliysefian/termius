# Contributing to SSHVault

Thanks for helping. Bug reports, ideas and pull requests are all welcome.

## Reporting bugs and ideas

[Open an issue](https://github.com/aliysefian/termius/issues) with:

- what you did, what you expected, and what happened instead;
- your SSHVault version (**Settings → Updates**) and operating system.

**Never paste passwords, private keys, vault files or host names you'd
rather keep private.** Replace them with placeholders. Security problems go
through [SECURITY.md](SECURITY.md), not public issues.

## Pull requests

1. For anything larger than a small fix, open an issue first so we can agree
   on the approach.
2. Set up the project as described in
   [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
3. Keep changes focused, and add or update tests for what you change.
4. Before you push, make sure these pass:

   ```sh
   pnpm check && pnpm test
   cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
   ```

5. Describe what changed and why in the pull request.

Changes to the vault format, cryptography, key handling or anything that
decides what to trust get extra review. Please explain the security
reasoning in the pull request.

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
