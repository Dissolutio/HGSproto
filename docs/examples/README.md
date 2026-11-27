# M3 dice demo

`m3-dice-demo.setup.json` is a version 2 setup named **M3 dice demo**.
Each seat has two units with attack 3, defense 3, health 4/4, and damage
per unblocked strike 1. The die has three strike faces, two block faces,
and one blank. Shadow is tagged `concealment` and adds one defense die.
These are PM-approved demo values, not balance recommendations. The built-in
sandbox remains the version 1 game.

> Layout notes (kept here because JSON cannot contain comments):
> Five single hexes form a lane at `(q, r) = (0, 0)` through `(4, 0)`.
> Every tile is at altitude 0; only q=3 is Shadow.
> Automatic placement puts P1's units at q=0 and q=1, and P2's at q=3 and q=4.
> Move `UNIT_P1_2` to q=2 to attack the Shadow defender. After that defender
> is removed, move to q=3 to attack the grass defender at q=4.
> Both moves are adjacent and stay at altitude 0. The height bonus is dormant.

`goliath-v2.profile.json` is a schema 2 unit card named **Goliath**
(`com.community.goliath`), with health 12/12 and the same combat values.
The demo's maximum-health cap is 10, so deploying Goliath requires an
explicit house-rule allowance; health is never silently reduced.

## Browser walkthrough — available after B8

These steps need a human browser check; they have **NOT EXERCISED** browser
status in B9. Core tests exercise the shipped files, not the browser UI.

1. Use **Load setup** to load `m3-dice-demo.setup.json`.
2. Before taking any action (which locks setup), import
   `goliath-v2.profile.json`.
3. Try to deploy Goliath into a unit slot at starting health **12**.
   The core should refuse it with a warning about maximum health 12 exceeding 10,
   without changing the match.
4. Allow it **as a house rule**. Confirm health stays 12/12 and a persistent
   house-rule notice appears. Export the setup and check that it contains
   exactly one allowance for that slot with 12/12, but no seed.
5. Reload the original demo for the combat walkthrough. Move along the lane
   described above and inspect attacks against Shadow and grass: Shadow adds
   one defense die; grass adds none. Skip the other seat's actions as needed.
6. Save during play, restore, and confirm health and the last attack do not
   change. Continue playing to a winner.

The core tests replay the same legal tokens with a fixed seed and check every
attack report, then save/restore mid-match and check an identical finish.
Loading an invalid rule is also tested to leave a running match unchanged.
Setup exports describe starting conditions and house rules, not a particular
seeded run; saved matches retain the seed and draw counter without re-rolling,
healing, or rescaling.
