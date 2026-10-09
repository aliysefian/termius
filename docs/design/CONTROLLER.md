# Design: an optional always-on controller (SSHV-021)

Status: proposal for decision. No code. It must never be required: the desktop app keeps working alone, offline, with no account.

## The problem it would solve

Three things in SSHVault stop when the window is closed, and that is by design today: monitoring and alerts (they poll while the
app is open), scheduled runbooks (they run only while it is open), and any record of who ran what. A person who wants to be told at
3 a.m. that a disk filled, or wants a nightly runbook to run with the laptop shut, needs something that is always on.

## What a controller is

A small program the owner runs on a machine they control. It polls hosts, runs schedules, evaluates alert rules, and sends
notifications. The desktop app is its console. It is **not** a hosted service and there is **no cloud account**.

## The hard part: credentials

An always-on thing that can log in to hosts needs credentials while nobody is there to unlock a vault. That is the whole risk.
Options, safest first:

| Option | How | Cost |
|---|---|---|
| **A. SSH certificates, short-lived** | The controller holds a CA-signed certificate valid for hours, renewed by a signing step the owner controls; hosts trust the CA | Needs a CA and host configuration; best blast radius |
| **B. A dedicated low-privilege account** | One key for a read-only monitoring user on each host, separate from anyone's personal key | Simple; the key lives on the controller, so the controller is a target |
| **C. Hand the controller the vault** | Controller unlocks the same vault | **Rejected**: it would hold every key and the master secret on an always-on machine |

Recommendation: B first (monitoring only, read-only account, no sudo), A for anything that changes hosts. Never C.

## Trust boundaries

Desktop to controller: mutual TLS with a device certificate issued at enrollment (a one-time code shown on the controller and typed
in the app). The controller exposes one authenticated port and nothing else. Controller to hosts: ordinary SSH with host-key
verification exactly as the app does it (no bypass, no trust-on-first-use without a person). Stored state: the controller keeps its
own small database (schedules, rules, results), encrypted at rest with a key that is itself not on the same disk where practical.

## What it must not do

Run a command that a person has not approved at definition time; hold personal keys; accept a plugin; reach the internet except to
send the notifications the owner configured; exist as the only way to use the app.

## Phases

1. Monitoring and alerts only, read-only account, notifications by a webhook the owner configures. Smallest useful thing.
2. Scheduled runbooks restricted to a list of runbooks marked safe for unattended use, run through the same engine.
3. An audit log the controller writes and members cannot edit.

## Decisions needed from the owner

1. Is a controller in scope at all, or is "keep the app open" acceptable? Phase 1 alone is a substantial project (a service, an
   enrollment protocol, packaging for several systems).
2. If yes: which credential option, B first as recommended?
3. Packaging: a single static binary and a container image, or a system package? Which operating systems for the controller?
4. Notification channel(s) to support, since each one sends host names off the owner's network.
