# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Client structure

- `src/HexGameCockpit.tsx` composes the page from game UI components.
- `src/game/hooks/useGameRuntime.ts` owns WASM and WebSocket session lifecycle,
  referee actions, turn/selection state, and runtime notifications.
- `src/game/components/` contains the 3D board, turn HUD, inspector, profile
  controls, and network portal. These components receive state and callbacks
  rather than accessing the engine directly.
- `src/game/model.ts`, `constants.ts`, and `types.ts` centralize game-state
  normalization, shared configuration, and feature types.

## Turn HUD and attack controls

- The permanent turn HUD shows your seat and color (P1 blue, P2 red), the active
  seat, phase, and **Your Turn** or **Waiting for Opponent**. Before host/join,
  network mode explicitly shows an unassigned seat. Offline actions are disabled.
- WASM mode is a local hotseat sandbox: you control whichever seat is active,
  and the HUD updates its seat and color after handover.
- In movement, click a friendly unit, then an adjacent unoccupied hex.
- In attack, click any active friendly unit. All adjacent destructible enemies
  on the same level receive red hex rings, glowing markers, and red TARGET
  health labels. Click a highlighted enemy (or its occupied hex) to attack.
  Friendly units and out-of-range enemies are never attack targets.
- Attack damage scales with maximum health: every 4 health grants 1 damage
  (minimum 1), so the default 8-health Hero deals 2 damage.
- Click the selected unit again, use **Cancel selection**, press **Escape**, or
  right-click the board to deselect. Inspection remains available while waiting.
- **Skip movement** enters attack. **Skip Attack / End Turn** skips attack and
  hands over to the opponent after the referee confirms the end phase.
  After executing an attack, use **End Turn** to hand over.
- Network actions are disabled while awaiting confirmation; a referee error or
  disconnect cancels a pending skip/handover rather than sending an end token.

### Browser smoke checklist

Run `npm run dev` from this directory and use the URL printed by Vite.

1. In WASM mode, confirm P1/Blue, Your Turn, and movement in the HUD.
2. Move `UNIT_P1_2` from `Q-1R2Z0` to the empty `Q0R2Z0` hex. Confirm attack phase.
3. Select that unit again: only `HERO_P2` and `UNIT_P2_2` should be red targets.
   Verify Escape, right-click, the cancel button, and clicking the selected unit
   clear selection and target highlights.
4. Reselect and click `UNIT_P2_2`. Confirm its health drops from 4/4 to 3/4,
   the phase becomes end, and End Turn hands over to P2/Red in movement.
5. Skip movement, then Skip Attack / End Turn. Confirm P1/Blue receives movement
   without any additional damage. An 8-health Hero attack should reduce a target
   by 2 health.
6. With two native network clients, verify the inactive seat sees Waiting for
   Opponent and disabled actions. Confirm skip/handover waits for the referee,
   and rejected actions or a disconnect do not cause an extra end-turn action.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
