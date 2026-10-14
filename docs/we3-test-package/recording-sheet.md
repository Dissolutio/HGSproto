# WE-3 Recording Sheet

Use one row per attempt, including failed, timed-out-at-observation-cutoff, and
not-exercised attempts. Record times with timezone. Record the firewall state
again for every row; do not write only “same” unless the earlier row/snapshot
is identified.

| Attempt number | Date and time (timezone) | Scenario | Which machine hosted | Machine 1 OS / network (home router or hotspot) | Machine 2 OS / network (home router or hotspot) | Machine 1 firewall rule state | Machine 2 firewall rule state | Join succeeded? | Seconds to success/failure/observation cutoff | What tester saw on failure | Stage on screen where it failed | Machine 1 log file name(s) | Machine 2 log file name(s) | Free-text observations |
|---|---|---|---|---|---|---|---|---|---:|---|---|---|---|---|
| | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | |

## Batch setup (complete once, update if anything changes)

| Field | Machine 1 | Machine 2 |
|---|---|---|
| Exact OS release/build | | |
| Browser and version | | |
| Rust and Cargo versions | | |
| Node and npm versions | | |
| wasm-pack version | | |
| Test commit (`git rev-parse HEAD`) | | |
| Network type/provider | Home broadband / router: | Phone hotspot / cellular carrier: |
| VPN or tunnel active? (must be no) | No / explain: | No / explain: |
| Existing firewall rule name, direction, action, profile, enabled state | | |
| Firewall prompt shown? Exact app/publisher/profiles/button clicked | | |
| Diagnostic log directory | | |

For each connection attempt, include the host and guest log filename(s) for that
attempt. Never paste the connection ticket, node ID, or match ID into this
sheet. Attach screenshots only after checking that they do not show the ticket.

## Short-match and recovery observations

For Scenario B, copy this small turn checklist for each turn:

| Turn | Acting seat/color | Phase at start | Move through 3D click (unit/from/to) | Attack through 3D click (attacker/target, HP before→after) | Skip control used | Host and guest board/health/active-seat agree? | Board click worked before health labels appeared? |
|---|---|---|---|---|---|---|---|
| | | | | | | | |

For Scenario C, record whether the host was idle or had just accepted an action
when the guest began rejoining, and compare the action count, seat, unit
positions, and health immediately before disconnect and after recovery.
