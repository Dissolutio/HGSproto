# WE-3 Recording Sheet

Use one row per attempt, including failed, timed-out-at-observation-cutoff, and
not-exercised attempts. Record times with timezone. Record the firewall state
again for every row; do not write only “same” unless the earlier row/snapshot
is identified.

| Attempt number | Date | Time of day (local) | Scenario | First peer join since gateway start? (host / guest: yes/no) | Gateway restarted since previous attempt? (machine/no) | Which machine hosted | Machine 1 OS / network | Machine 2 OS / network | Machine 1 firewall rule state | Machine 2 firewall rule state | Hotspot carrier and signal strength | Join succeeded? | Seconds to success/failure/observation cutoff | What tester saw on failure | Stage on screen where it failed | Machine 1 log file name(s) | Machine 2 log file name(s) | Free-text observations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---:|---|---|---|---|---|
| | | | | | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | | | | |
| | | | | | | | | | | | | | | | | | |

## Batch setup (complete once, update if anything changes)

| Field | Machine 1 | Machine 2 |
|---|---|---|
| Exact OS release/build | | |
| Browser and version | | |
| Rust and Cargo versions | | |
| Node and npm versions | | |
| wasm-pack version | | |
| Test commit (`git rev-parse HEAD`) | | |
| Session start date/time (local) | | |
| Network type/provider (name only; no addresses) | Home broadband provider/router name: | Phone hotspot cellular carrier name: |
| VPN or tunnel active? (must be no) | No / explain: | No / explain: |
| Phone Wi-Fi off; hotspot using cellular? | N/A | Yes / no |
| Phone signal strength | N/A | |
| Time of day and other devices heavily using either network | | |
| Existing firewall rule name, direction, action, profile, enabled state | | |
| Firewall prompt shown? Exact app/publisher/profiles/button clicked | | |
| Diagnostic log directory | | |

For each connection attempt, include the host and guest log filename(s) for that
attempt. Never paste the connection ticket, node ID, or match ID into this
sheet. Attach screenshots only after checking that they do not show the ticket.

## Short-match observations

For Scenario B, copy this small turn checklist for each turn:

| Turn | Acting seat/color | Phase at start | Move through 3D click (unit/from/to) | Attack through 3D click (attacker/target, HP before→after) | Skip control used | Host and guest board/health/active-seat agree? | Board click worked before health labels appeared? |
|---|---|---|---|---|---|---|---|
| | | | | | | | |

## Scenario C1 — browser-to-gateway recovery

| C1 attempt | Host idle or action just accepted | Browser reloaded in same tab/origin? | Seconds until browser-to-gateway recovery or cutoff | Peer status before/after | Seat, positions, health, and action state agree? | Host/guest log filenames |
|---|---|---|---:|---|---|---|
| | | | | | | |
| | | | | | | |
| | | | | | | |
| | | | | | | |
| | | | | | | |

## Scenario C2 — replacement peer

| C2 attempt | What dropped the peer connection | Host state: idle or just accepted action | Seconds until replacement connected or cutoff | State before drop (seat/actions/positions/health) | State after replacement (seat/actions/positions/health) | State preserved? | Host/guest log filenames | Observations |
|---|---|---|---:|---|---|---|---|---|
| | Guest gateway stopped/restarted | | | | | | | |
| | Guest gateway stopped/restarted | | | | | | | |

Complete at least 20 C2 attempts: at least 10 while the host is idle and at
least 10 immediately after the host accepted an action. Do not record any
connection ticket, node ID, or match ID.

## Scenario G — ticket outcomes

| Attempt | Case (G1/G2/G3) | Exact message shown | Interface responsive? | Seconds until message or 120-second cutoff | Peer-attempt log expected/observed? | Log filename(s) | Observations |
|---|---|---|---|---:|---|---|---|
| | | | | | | | |
| | | | | | | | |
| | | | | | | | |
