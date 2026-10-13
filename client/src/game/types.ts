import type { RefObject } from "react";

export type Position = { q: number; r: number; z: number };

export type EngineComponent = {
  type?: string;
  data?: {
    inventory_id?: string;
    face_texture?: string;
    side_texture?: string;
    labels?: string[];
    current_life?: number;
    max_life?: number;
  };
  Terrain?: {
    inventory_id?: string;
    face_texture?: string;
    side_texture?: string;
  };
  TerrainType?: { terrain?: string };
  Tags?: { labels?: string[] };
  Destructible?: { current_life?: number; max_life?: number };
  destructible?: { current_life?: number; max_life?: number };
};

export type EngineEntity = {
  id?: string;
  spatial?: Position;
  visual_primitive?: string;
  display_name?: string;
  components?: Record<string, EngineComponent> | EngineComponent[];
};

export type EngineState = {
  context?: {
    match_id?: string;
    active_player?: string;
    turn_phase?: string;
    setup_locked?: boolean;
    winner?: string;
  };
  entities?: Record<string, EngineEntity> | EngineEntity[];
};

export type RenderEntity = EngineEntity & { id: string; spatial: Position };
export type TerrainData = RenderEntity & {
  faceTextureUrl: string;
  sideTextureUrl: string;
};

export type Mode = "network" | "wasm";
export type ConnectionState = "connecting" | "connected" | "offline" | "ready";
export type PlayerSeat = "P1" | "P2";
export type RecoverySession = { matchId: string; managerId: string };

export type ModProfileSummary = {
  modId: string;
  displayName: string;
  visualPrimitive: string;
  currentHealth: number;
  maxHealth: number;
};

export type HealthLabelElements = RefObject<Map<string, HTMLDivElement>>;
