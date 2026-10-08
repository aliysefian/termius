# Completed tasks

Nothing has met its acceptance criteria with recorded test evidence yet.

Shipped earlier, before this backlog (not re-verified here): 0.26.3 fixed the app closing when opening container or
Kubernetes logs (`containers/mod.rs:534`, `kube.rs:399`). It is tracked under SSHV-004, which stays open because the
logging/panic-hook/shutdown work and a regression guard are not done, and because the fix has not been run on a release
build.
