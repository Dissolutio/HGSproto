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

## Stop rules

- If the first three attempts of any scenario fail in the same way, stop that
  scenario, keep the logs and worksheet rows, and report to the PM before
  continuing.
- If any step cannot be carried out as written, stop and report instead of
  improvising.
- Never retry a failed or pending attempt.

## Session plan

- **Session 1:** Scenario A, including the five gateway restarts; then B; then
  G1, G2, and G3.
- **Session 2:** C2; then D; then F; then C1.
- **Scenario E** is optional and is run only if a third network pairing is
  actually available.

At the start of each session, repeat the **Before the first attempt**
checklist. Record the local time of day and the commit (`git rev-parse HEAD`)
on both machines.

## Before the first attempt

- [ ] Confirm the phone's Wi-Fi is off, so its hotspot is using cellular data.
- [ ] Confirm the Linux laptop is on home broadband and is not connected to
      the phone hotspot.
- [ ] Confirm the laptops use different providers or routers. Record provider
      or router names only; do not record addresses.
- [ ] Record the phone's cellular carrier and signal strength (for example,
      signal bars out of the phone's displayed total).
- [ ] Note that hotspot data use will be significant during this test.
- [ ] Record the local time of day and any other devices heavily using either
      network. Update the time and observations for each attempt.

## A. Fresh join — at least 20 attempts

1. Complete the checklist above. Start both gateways and browser clients as
   described in the setup guide.
2. Before each attempt after the first, end the previous match on both clients:
   the host clicks **Leave Match & Reset Gateway**, then the guest clicks the
   same control. Wait for both pages to show the reset/fresh local state,
   select **Live Network Mode** again on each page, and wait for both local
   gateway statuses to show connected. Do not stop the gateway processes for
   this reset.
3. During the batch, restart a gateway at least five times total, with at least
   two restarts on each laptop. Do this only between attempts, after resetting
   the previous match. In that gateway's terminal, press **Ctrl+C**, then run
   the same diagnostic gateway command from the setup guide and wait for
   `P2P gateway ready`. Leave the Vite server running. Record which laptop's
   gateway restarted. These restarts ensure both first-join and later-join
   conditions are represented.
4. Alternate the host: Machine 1 hosts odd-numbered attempts and Machine 2
   hosts even-numbered attempts. For each attempt, record whether it is the
   first peer join since the latest gateway start on the host and on the guest.
5. The host clicks **Host Match** and privately shares the complete, current
   ticket. The guest pastes it into **Host connection ticket** and clicks
   **Join Match**. Never put the ticket in the worksheet, a screenshot, or a
   shared log.
6. Record the join result, elapsed time, both log filenames, route/stage
   evidence, and the state shown on both screens. If an attempt fails or is
   still pending at 120 seconds, preserve it as a result; do not retry that
   ticket.
7. Reset both clients as in step 2 before starting the next fresh match.

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

## C1. Browser reload recovery — at least 5 attempts

This checks recovery of the **browser-to-local-gateway link**. It does not drop
the guest gateway's peer connection or test a replacement peer.
C1 is deliberately small because it tests only the browser-to-gateway link.

1. Start and join a fresh match. Record both boards, active seat, positions,
   health, and accepted-action evidence.
2. Keep both gateway processes running. Reload the same guest browser tab
   (`Ctrl+R` / `F5`) on the same origin so its session storage is preserved.
3. When the recovery banner appears, click **Attempt Graceful Reconnection**.
   Record the browser-to-gateway result and whether the peer connection and
   match state remain intact. A transient peer-status notice alone does not
   establish that a new peer connection was made.
4. Repeat for at least 5 attempts, using a fresh match each time and the
   Scenario A reset procedure between attempts. Do not retry an individual
   failed or pending attempt.

## C2. Guest peer replacement — at least 20 attempts

This scenario is based on code reading and has not been run. If the host
refuses the replacement guest or the join fails the same way on the first three
attempts, apply the stop rules.

This requires an actual guest gateway shutdown and a new **Join Match**. A
browser reload alone is not a peer replacement. The replacement gateway has a
new peer identity; the host gateway and its current ticket must remain active.
Complete at least 20 attempts: at least 10 while the host is idle and at least
10 immediately after the host has accepted an action.

1. For each attempt, start a fresh match and join as in Scenario A. Keep the
   host gateway process running. Keep the current host ticket private and
   available to paste again; do not record it.
2. For an **idle** attempt, do not make a game action immediately before the
   guest gateway is stopped. For an **after-action** attempt, have the host
   accept one legal action, confirm that the resulting state is visible on
   both screens, and then immediately proceed.
3. On the guest laptop, press **Ctrl+C** in the gateway terminal while leaving
   the host gateway running. This closes the guest endpoint and is the step
   that drops the peer connection. Do not substitute a browser reload or a
   short network interruption. Record when the host reports the peer
   disconnected and the pre-drop match state.
4. Restart the guest gateway with the diagnostic command from the setup guide.
   Wait for it to print `P2P gateway ready` and for the guest browser to show
   its local gateway connected. If the old in-memory session is reported
   expired, wait for the page to reconnect to a fresh local session.
5. On the guest, paste the same still-current host ticket into **Host
   connection ticket** and click **Join Match**. This uses the normal join
   flow to create a new peer connection; it is not the graceful-reconnection
   button. Stop timing at success, visible failure, or the 120-second
   observation cutoff. Do not retry a failed or pending attempt.
6. On success, confirm the guest is P2/Red and compare the host snapshot,
   positions, health, seat, and accepted-action state with the pre-drop state.
   Record both log filenames and observations on both screens.
7. End/reset both clients using the Scenario A procedure before the next
   attempt. Alternate the idle and after-action conditions until each has at
   least 10 results. Do not count C1 browser reloads toward the C2 total.

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

## G. Ticket rejection and unavailable host

For each G attempt, start the timer when the guest clicks **Join Match**. Record
the exact message, whether the page still responds to a harmless action such as
collapsing and reopening the portal, and the seconds until a message appears.
If there is no message by 120 seconds, record `still pending at 120s`. Do not
submit the ticket again or retry a pending attempt.

### G1. Missing suffix

1. On the guest, select Live Network Mode and open the portal.
2. Paste only the host Node ID, omitting `@` and the match-ID suffix, then click
   **Join Match**.
3. Record the exact message and response time. This client-side validation
   should reject the input before a peer connection is attempted, so no
   peer-attempt log is expected.

### G2. Ticket for a match the host ended

1. Host a match and privately retain the complete ticket.
2. On the host, click **Leave Match & Reset Gateway** and wait for the reset.
   Keep the guest gateway running.
3. On the guest, submit the retained, correctly formatted ticket once. Record
   the exact result using the timing and responsiveness instructions above.

### G3. Host gateway not running

1. Host a match and privately retain the complete, correctly formatted ticket.
2. Press **Ctrl+C** in the host gateway terminal and verify that the host
   gateway has stopped. Do not stop the guest gateway.
3. On the guest, submit the ticket once. Record the exact result using the
   timing and responsiveness instructions above. Restart the host gateway
   after recording the outcome. Do not retry a pending attempt.

## Analysis plan

For each scenario and network pairing, report attempts, successes, failures,
and test-level pending/observation-cutoff outcomes separately. For completed
connections, calculate median and worst connection time. For every failure or
pending attempt, report each side's last route observation and last logged
stage; label unknown values as unknown. Keep the route-classification sample
time caveat explicit: route changes are timestamped when first observed by the
one-second sampler, not at the exact underlying transition. Report C1
browser-to-gateway recovery separately from C2 replacement-peer attempts; do
not count C1 as peer reconnection evidence. If C1 has fewer than 20 attempts,
report its results as an observation, not a rate.

Compare the cross-network sample with the existing single-machine default-mode
recovery measurements, explicitly stating their sample sizes and that the
recovery failure's cause remains unknown. Do not claim a route caused a failure
unless the logs show that evidence.
