# WE-3 Quick Proof

This is a small proof that the cross-network path can work, **not a reliability
measurement**. The full test script is kept for later in
[test-script.md](./test-script.md). Use the [recording sheet](./recording-sheet.md)
for every attempt.

## Before starting

- [ ] Phone Wi-Fi is off, so the hotspot uses cellular data.
- [ ] Linux laptop is on home broadband, not the phone hotspot.
- [ ] Confirm the laptops use different providers or routers. Record provider
      or router names only, never addresses.
- [ ] Record the phone's cellular carrier and signal strength (for example,
      signal bars out of the phone's displayed total).
- [ ] Note that hotspot data use will be significant.
- [ ] Record the local time of day and any other devices heavily using either
      network.
- [ ] Both machines report the same commit with `git rev-parse HEAD`, and both
      worktrees are clean.
- [ ] Start each native gateway with the diagnostics feature enabled, as shown
      in the README.
- [ ] Start each browser server in local-only mode with `--host 127.0.0.1`, as
      shown in the README.

Use the README's ticket-handling rules. Never put a ticket or node ID in the
sheet, logs, screenshots, or repository.

## Part 1 — fresh joins

Complete at least **5** fresh joins; do up to 10 if time allows. Alternate
which laptop hosts. For each attempt, end the previous match and reset both
clients using the exact Scenario A procedure in [test-script.md](./test-script.md),
then start a fresh match. Record every attempt on the recording sheet; do not
retry a failed or pending attempt. If the first 3 joins fail in the same way,
stop and report to the PM.

## Part 2 — one short turn

Use each player's own page for that laptop's actions. Click the 3D board to
make one legal move and one legal attack, then end the turn using the available
turn control. After each accepted action, confirm both screens agree on health,
positions, and active seat. Record whether the 3D board could be clicked before
the health labels appeared.

## Part 3 — one peer replacement

Follow Scenario C2 in [test-script.md](./test-script.md): stop the guest
gateway with **Ctrl+C**, restart it, and rejoin with the same current host
ticket. Compare health, positions, and active seat before and after. This has
not been run before; a failure is an important finding, not a mistake.

## Finish and report confusion

Stop every gateway and browser server on both machines with **Ctrl+C**. Then
collect results as described in the README, fill in the recording sheet, and
verify that the bundle contains no ticket, node ID, or screenshot showing one.

### What to write down if anything is confusing

Note any step where you paused or guessed, including what the screen showed.
If a step cannot be carried out as written, stop and report instead of
improvising.
