export type CombatState = {
  combat?: null;
  die: { faces: string[] };
  terrain_types: { id: string; name: string; tags: string[] }[];
  house_rules: { name: string }[];
  last_attack: {
    sequence: number;
    attacker: string;
    defender: string;
    attack: { base: number; height_bonus: number; total: number; faces: number[]; strikes: number };
    defense: { base: number; height_bonus: number; terrain_bonus: number; terrain: string; total: number; faces: number[]; blocks: number };
    unblocked_strikes: number;
    damage_per_strike: number;
    damage: number;
    defender_health_before: number;
    defender_health_after: number;
    defender_removed: boolean;
  } | null;
};

export function seedHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function isV2Setup(text: string): boolean {
  try {
    return JSON.parse(text)?.setup_version === 2;
  } catch {
    return false;
  }
}

export function houseRuleNames(state: CombatState | null): string[] {
  return state?.house_rules.map((rule) => rule.name) ?? [];
}

export function formatAttackReport(state: CombatState | null): string[] {
  if (!state || state.combat === null) return [];
  const attack = state.last_attack;
  if (!attack) return ["No attacks yet"];
  const terrain = state.terrain_types.find((item) => item.id === attack.defense.terrain);
  const terrainLabel = terrain
    ? `${terrain.name}${terrain.tags.length ? ` (${terrain.tags.join(", ")})` : ""}`
    : attack.defense.terrain;
  const faces = (indices: number[]) => indices.map((index) => state.die.faces[index]).join(", ");
  return [
    `Attack #${attack.sequence}: ${attack.attacker} → ${attack.defender}`,
    `Attack dice: ${faces(attack.attack.faces)}`,
    `Attack: ${attack.attack.base} base, +${attack.attack.height_bonus} height = ${attack.attack.total}; strikes: ${attack.attack.strikes}`,
    `Defense dice: ${faces(attack.defense.faces)}`,
    `Defense: ${attack.defense.base} base, +${attack.defense.height_bonus} height, +${attack.defense.terrain_bonus} terrain: ${terrainLabel} = ${attack.defense.total}; blocks: ${attack.defense.blocks}`,
    `Unblocked strikes: ${attack.unblocked_strikes}; damage per strike: ${attack.damage_per_strike}; damage: ${attack.damage}`,
    `Defender health: ${attack.defender_health_before} → ${attack.defender_health_after}${attack.defender_removed ? " (removed)" : ""}`,
  ];
}
