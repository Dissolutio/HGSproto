import assert from "node:assert/strict";
import test from "node:test";
import { seedHex, isV2Setup, houseRuleNames, formatAttackReport } from "../.test-build/game/diceDisplay.js";

test("seed bytes are encoded as lowercase, zero-padded hex", () => {
  assert.equal(seedHex(new Uint8Array(8)), "0000000000000000");
  assert.equal(seedHex(new Uint8Array(8).fill(255)), "ffffffffffffffff");
  assert.equal(seedHex(Uint8Array.from([0, 1, 15, 16, 127, 128, 254, 255])), "00010f107f80feff");
});

test("only version 2 JSON takes the new load path", () => {
  assert.equal(isV2Setup('{"setup_version":2}'), true);
  for (const text of ['{"setup_version":1}', "{}", "invalid", "null"]) {
    assert.equal(isV2Setup(text), false);
  }
});

test("house-rule names are shown in order, or not at all", () => {
  assert.deepEqual(houseRuleNames({ house_rules: [{ name: "First" }, { name: "Second" }] }), ["First", "Second"]);
  assert.deepEqual(houseRuleNames({ house_rules: [] }), []);
  assert.deepEqual(houseRuleNames(null), []);
});

test("attack report displays the core's rolls and results", () => {
  const state = {
    combat_version: 1,
    die: { faces: ["strike", "strike", "strike", "block", "block", "blank"] },
    terrain_types: [{ id: "shadow", name: "Shadow", tags: ["concealment"] }],
    house_rules: [], attacks_resolved: 1,
    last_attack: {
      sequence: 1, attacker: "UNIT_P1_1", defender: "UNIT_P2_1",
      attack: { base: 3, height_bonus: 0, total: 3, faces: [3, 5, 2], counts: { strike: 1, block: 1, blank: 1 }, strikes: 1 },
      defense: { base: 2, height_bonus: 0, terrain_bonus: 1, terrain: "shadow", total: 3, faces: [2, 2, 5], counts: { strike: 2, block: 0, blank: 1 }, blocks: 0 },
      unblocked_strikes: 1, damage_per_strike: 1, damage: 1,
      defender_health_before: 1, defender_health_after: 0, defender_removed: true,
      draws: { from: 0, to: 6 },
    },
  };
  assert.deepEqual(formatAttackReport(state), [
    "Attack #1: UNIT_P1_1 → UNIT_P2_1",
    "Attack dice: block, blank, strike",
    "Attack: 3 base, +0 height = 3; strikes: 1",
    "Defense dice: strike, strike, blank",
    "Defense: 2 base, +0 height, +1 terrain: Shadow (concealment) = 3; blocks: 0",
    "Unblocked strikes: 1; damage per strike: 1; damage: 1",
    "Defender health: 1 → 0 (removed)",
  ]);
  assert.deepEqual(formatAttackReport({ combat: null }), []);
  assert.deepEqual(formatAttackReport({ ...state, last_attack: null }), ["No attacks yet"]);
});
