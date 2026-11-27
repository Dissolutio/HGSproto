import type {
  EngineComponent,
  EngineEntity,
  EngineState,
  ModProfileSummary,
  PlayerSeat,
  Position,
  RenderEntity,
  TerrainData,
} from "./types";

export function componentList(entity: EngineEntity): EngineComponent[] {
  return Array.isArray(entity.components)
    ? entity.components
    : Object.values(entity.components ?? {});
}

export function findComponent(entity: EngineEntity, name: string): EngineComponent | undefined {
  return componentList(entity).find(
    (component) => component.type === name || name in component,
  );
}

export function terrainTextures(entity: EngineEntity) {
  const terrain = findComponent(entity, "Terrain");
  return {
    faceTextureUrl:
      terrain?.data?.face_texture ?? terrain?.Terrain?.face_texture ?? "grass_face",
    sideTextureUrl:
      terrain?.data?.side_texture ?? terrain?.Terrain?.side_texture ?? "earth_side",
  };
}

export function unitLabels(entity: EngineEntity): string[] {
  const tagged = findComponent(entity, "Tags");
  return tagged?.data?.labels ?? tagged?.Tags?.labels ?? [];
}

export function unitSeat(entity: EngineEntity): PlayerSeat | null {
  const labels = unitLabels(entity);
  const playerOne = labels.includes("P1");
  const playerTwo = labels.includes("P2");
  return playerOne !== playerTwo ? (playerOne ? "P1" : "P2") : null;
}

export function isAdjacent(left: Position, right: Position): boolean {
  const dq = left.q - right.q;
  const dr = left.r - right.r;
  return left.z === right.z && (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2 === 1;
}

export function unitHealth(entity: EngineEntity): { currentLife: number; maxLife: number } | null {
  const destructible = findComponent(entity, "Destructible");
  const currentLife =
    destructible?.data?.current_life ??
    destructible?.Destructible?.current_life ??
    destructible?.destructible?.current_life;
  const maxLife =
    destructible?.data?.max_life ??
    destructible?.Destructible?.max_life ??
    destructible?.destructible?.max_life;
  if (
    typeof currentLife !== "number" ||
    !Number.isFinite(currentLife) ||
    typeof maxLife !== "number" ||
    !Number.isFinite(maxLife)
  ) {
    return null;
  }
  return { currentLife, maxLife };
}

export function entitiesFromState(state: EngineState | null): RenderEntity[] {
  if (!state?.entities) return [];
  const entries = Array.isArray(state.entities)
    ? state.entities.map((entity, index) => [entity.id ?? String(index), entity] as const)
    : Object.entries(state.entities);

  return entries.flatMap(([key, entity]) => {
    if (!entity?.spatial || !entity.components) return [];
    return [{ ...entity, id: entity.id ?? key, spatial: entity.spatial }];
  });
}

export function parseState(serializedState: string): EngineState {
  const parsed = JSON.parse(serializedState) as EngineState | { error?: string };
  if ("error" in parsed && parsed.error) throw new Error(parsed.error);
  return parsed as EngineState;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

export function profileHealth(profile: Record<string, unknown>) {
  const components = isRecord(profile.components) ? profile.components : null;
  const destructible = components?.destructible;
  const data =
    isRecord(destructible) && isRecord(destructible.data)
      ? destructible.data
      : destructible;
  const currentHealth = isRecord(data) ? data.current_life : undefined;
  const maxHealth = isRecord(data) ? data.max_life : undefined;
  if (
    typeof currentHealth !== "number" ||
    !Number.isInteger(currentHealth) ||
    typeof maxHealth !== "number" ||
    !Number.isInteger(maxHealth) ||
    currentHealth < 1 ||
    maxHealth < currentHealth
  ) {
    throw new Error("Each unit profile needs valid destructible current_life and max_life values");
  }
  return { currentHealth, maxHealth };
}

export function summarizeProfiles(profiles: unknown[]): ModProfileSummary[] {
  return profiles.map((profile): ModProfileSummary => {
    if (!isRecord(profile)) {
      throw new Error("Each mod profile must be a JSON object");
    }
    return {
      modId: typeof profile.mod_id === "string" ? profile.mod_id : "Unknown mod ID",
      displayName:
        typeof profile.display_name === "string" ? profile.display_name : "Unnamed profile",
      visualPrimitive:
        typeof profile.visual_primitive === "string"
          ? profile.visual_primitive
          : "Unknown primitive",
      ...profileHealth(profile),
    };
  });
}

export function terrainFromEntities(entities: RenderEntity[]): TerrainData[] {
  return entities.flatMap((entity): TerrainData[] => {
    if (!findComponent(entity, "Terrain")) return [];
    return [{ ...entity, ...terrainTextures(entity) }];
  });
}
