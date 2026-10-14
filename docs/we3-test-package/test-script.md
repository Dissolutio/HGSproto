# WE-3 Cross-Network Test Script

Follow [the package setup guide](./README.md) first. Keep the diagnostic gateway
and local-only browser server running on both machines throughout the batch.
Use the worksheet in [recording-sheet.md](./recording-sheet.md) for every
attempt. Do not change firewall rules, install a VPN/tunnel, or retry the same
failed attempt.

For a join attempt, start timing when the guest clicks **Join Match** and stop
at the success message or visible error. If it remains on **Connecting to Iroh
host…** at 120 seconds, record `still pending at 120s` as an observation cutoff,
not as a product-enforced timeout. Keep the pending log and do not retry that
attempt. Record route/stage from each local log without inferring a cause.

## A. Fresh join — at least 20 attempts

1. Start with a fresh host match. For odd-numbered attempts, Machine 1 hosts;
   for even-numbered attempts, Machine 2 hosts.
2. Record the firewall state on both machines. Confirm no VPN or tunnel is
   active.
3. On the host, select **Live Network Mode**, expand the portal, click
   **Host Match**, and copy the entire ticket, including `@browser_...`.
4. Send the ticket privately. On the guest's own local client, select Live
   Network Mode, paste the complete ticket, and click **Join Match**.
5. Record the time, visible status/error, success or observation cutoff, and
   both log filenames. Confirm P1/Blue on the host and P2/Red on the guest.
6. If the attempt fails or remains pending, preserve it as a result. Do not
   repeat that same ticket. For the next attempt, start a fresh host match and
   use the next attempt number.
7. Complete at least 20 attempts, alternating which laptop hosts.

## B. Play a short match

1. Use a successful Scenario A session. Before making a game action, inspect
   both pages and record the active seat, phase, unit health, and board state.
2. As soon as the network board is visible but before its health labels appear,
   click one visible friendly unit once (selection only; do not click a
   destination). Record whether the board was clickable, whether selection or
   inspection changed, and whether this happened before labels appeared. If no
   board is visible yet, record that and wait for the first labels before
   continuing.
3. Have P1 move one friendly unit by clicking the unit and a legal adjacent
   empty hex, then attack one highlighted adjacent enemy by clicking it.
   Confirm after each accepted action that both pages show the same position,
   health, active seat, and phase.
4. Repeat a legal move and attack for P2, then again for P1 and P2. Use actual
   3D board clicks for each move and attack. Record attacker/target HP before
   and after.
5. Demonstrate **Skip Movement** on one additional turn, then make a legal
   attack if available. On a later turn demonstrate **Skip Attack / End Turn**.
   These skip demonstrations do not replace the requirement that both seats
   each perform at least one move and one attack.
6. After every phase transition and turn handover, compare both pages. Record
   any disagreement, rejected action, delay, or visible error and attach the
   corresponding log names.

## C. Disconnect and rejoin — at least 20 attempts

1. Start and join a fresh match. Record both boards, active seat, positions,
   health, and the accepted-action count or other visible action evidence.
2. Use the same guest tab and reload it (`Ctrl+R` / `F5`) to preserve that tab's
   session storage. When the recovery banner appears, click **Attempt Graceful
   Reconnection**. Do not use **Leave Match & Reset Gateway**, which clears the
   recovery data.
3. For at least five attempts, begin rejoin while the host is idle. For at
   least five others, have the host complete and confirm an action, then reload
   the guest immediately. Alternate which laptop is guest over the batch.
4. Confirm the guest returns as the same seat and that accepted actions, health,
   and unit positions match the host. Check specifically for healing or a
   repeated attack. Record both log files and the final states.
5. Repeat with a fresh session until at least 20 rejoin attempts are recorded.
   Do not retry an individual failed or pending attempt.

## D. Host connection loss

1. Establish a live match and record both players' current state.
2. Only if the host can safely restore its own network connection, briefly
   disconnect the host laptop from its current network for 30 seconds using
   its normal OS network control. Do not disable the firewall, change a rule,
   restart the router, or interrupt other household equipment. If safely
   disconnecting is not possible, mark this scenario not exercised.
3. Restore the same network. Record what each screen showed during the outage
   and after connectivity returned, plus the logs and elapsed times. Do not
   infer recovery or route from the UI alone.

## E. Additional network pairings — at least 10 joins per pairing

For each additional pairing available, identify both network types and the
firewall state on both machines. Follow Scenario A for at least 10 fresh joins,
alternating hosts when possible. Keep each pairing's results distinct; do not
combine its counts with the home-router/cellular-hotspot baseline.

## F. Long idle — at least 15 minutes

1. Establish one joined match and record both states and log names.
2. Leave both clients and both gateways running, with no game actions, for at
   least 15 minutes.
3. Record whether either page disconnects or changes route status while idle.
   After the idle period, take one legal turn and confirm both pages agree on
   seat, phase, positions, and health.

## G. Bad or incomplete ticket

1. On the guest, select Live Network Mode and open the portal.
2. Paste only the host Node ID, omitting `@` and the match-ID suffix, then click
   **Join Match**.
3. Record the exact visible message and seconds to display. The expected client
   message is `Paste the complete host connection ticket (Node ID@match ID)`.
4. Confirm the host receives no peer connection. This invalid input is rejected
   in the client before a network attempt; do not expect a peer-attempt log for
   it.

## Analysis plan

For each scenario and network pairing, report attempts, successes, failures,
and test-level pending/observation-cutoff outcomes separately. For completed
connections, calculate median and worst connection time. For every failure or
pending attempt, report each side's last route observation and last logged
stage; label unknown values as unknown. Keep the route-classification sample
time caveat explicit: route changes are timestamped when first observed by the
one-second sampler, not at the exact underlying transition.

Compare the cross-network sample with the existing single-machine default-mode
recovery measurements, explicitly stating their sample sizes and that the
recovery failure's cause remains unknown. Do not claim a route caused a failure
unless the logs show that evidence.
