import type { ReactNode } from "react";
import type { RenderEntity } from "../types";
import { findComponent } from "../model";
import { formatAttackReport, houseRuleNames, type CombatState } from "../diceDisplay";

type InspectorPanelProps = {
  entity: RenderEntity | null;
  combatState?: CombatState | null;
  terrain?: RenderEntity[];
  children: ReactNode;
};

export function InspectorPanel({ entity, combatState, terrain = [], children }: InspectorPanelProps) {
  const combat = entity && findComponent(entity, "Combat")?.data;
  const tile = entity && terrain.find((item) =>
    item.spatial.q === entity.spatial.q && item.spatial.r === entity.spatial.r && item.spatial.z === entity.spatial.z);
  const terrainId = tile && findComponent(tile, "TerrainKind")?.data?.id;
  const terrainType = combatState?.terrain_types.find((item) => item.id === terrainId);
  const rules = houseRuleNames(combatState ?? null);
  return (
    <aside style={styles.inspector} aria-label="Entity component inspector">
      <h2 style={styles.title}>📊 HGS Real-Time Component Inspector</h2>
      {entity ? (
        <pre style={styles.content}>{JSON.stringify(entity, null, 2)}</pre>
      ) : (
        <p style={styles.empty}>Click a unit or hex pillar to inspect its components.</p>
      )}
      {combatState && entity && (
        <div style={styles.detail}>
          {combat && <p>Attack: {combat.attack} · Defense: {combat.defense} · Damage per strike: {combat.damage_per_strike}</p>}
          {terrainType && <p>Terrain: {terrainType.name}{terrainType.tags?.length ? ` (${terrainType.tags.join(", ")})` : ""}</p>}
        </div>
      )}
      {combatState && (
        <section style={styles.detail} aria-label="Combat">
          <h3>Combat</h3>
          {formatAttackReport(combatState).map((line, index) => <p key={index}>{line}</p>)}
        </section>
      )}
      {rules.length > 0 && (
        <section style={styles.detail} aria-label="House rules">
          <h3>House rules</h3>
          <ul>{rules.map((name, index) => <li key={index}>{name}</li>)}</ul>
        </section>
      )}
      {children}
    </aside>
  );
}

const styles = {
  inspector: {
    position: "absolute" as const,
    zIndex: 2,
    top: 12,
    right: 12,
    width: "min(340px, calc(100% - 24px))",
    maxHeight: "calc(100% - 24px)",
    overflowY: "auto" as const,
    boxSizing: "border-box" as const,
    padding: 14,
    border: "1px solid #4d6860",
    background: "rgba(8, 20, 18, 0.86)",
    color: "#e5efe8",
    boxShadow: "0 8px 24px #0006",
    backdropFilter: "blur(6px)",
  },
  title: {
    margin: "0 0 10px",
    fontSize: 13,
    fontWeight: 650,
    lineHeight: 1.4,
  },
  content: {
    margin: 0,
    fontSize: 11,
    fontFamily: "monospace",
    color: "#10b981",
    whiteSpace: "pre-wrap" as const,
    overflowWrap: "anywhere" as const,
  },
  empty: { margin: 0, color: "#9aac9e", fontSize: 12, lineHeight: 1.5 },
  detail: { marginTop: 12, fontSize: 12, overflowWrap: "anywhere" as const },
};
