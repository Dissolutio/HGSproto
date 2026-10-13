import type { PlayerSeat } from "./types";

export const PLAYER_COLORS: Record<PlayerSeat, string> = {
  P1: "#3b82f6",
  P2: "#ef4444",
};

export const SOCKET_URL = "ws://127.0.0.1:8080";
export const WASM_MATCH_ID = "wasm_sandbox";
export const TAB_ID_STORAGE_KEY = "hexgamesim.tab-id";
export const ACTIVE_MATCH_ID_STORAGE_KEY = "active_match_id";
export const CACHED_MANAGER_ID_STORAGE_KEY = "cached_manager_id";
export const HEX_RADIUS = 1;
export const PILLAR_HEIGHT = 0.42;

function mockStarterUnit(id: string, labels: string[], life: number) {
  return {
    id,
    components: {
      tags: { type: "Tags", data: { labels } },
      destructible: {
        type: "Destructible",
        data: { current_life: life, max_life: life },
      },
    },
  };
}

export const MOCK_MAP_JSON = JSON.stringify({
  hexMap: { name: "Local WASM Sandbox" },
  boardPieces: [
    {
      uid: "sandbox-g7",
      inventoryID: "g7",
      altitude: 0,
      rotation: 0,
      pieceCoords: { q: 0, r: 0, s: 0 },
    },
  ],
  starterUnits: [
    mockStarterUnit("UNIT_P1_1", ["Unit", "P1"], 4),
    mockStarterUnit("UNIT_P1_2", ["Unit", "P1"], 4),
    mockStarterUnit("UNIT_P2_1", ["Unit", "P2"], 4),
    mockStarterUnit("UNIT_P2_2", ["Unit", "P2"], 4),
    mockStarterUnit("HERO_P1", ["Unit", "P1", "Hero"], 8),
    mockStarterUnit("HERO_P2", ["Unit", "P2", "Hero"], 8),
  ],
});

export function getTabId(): string {
  const existingTabId = window.sessionStorage.getItem(TAB_ID_STORAGE_KEY);
  if (existingTabId) return existingTabId;
  const tabId = `tab_${window.crypto.randomUUID().replace(/-/g, "")}`;
  window.sessionStorage.setItem(TAB_ID_STORAGE_KEY, tabId);
  return tabId;
}

export function readRecoverySession() {
  const matchId = window.sessionStorage.getItem(ACTIVE_MATCH_ID_STORAGE_KEY);
  const managerId = window.sessionStorage.getItem(CACHED_MANAGER_ID_STORAGE_KEY);
  if (!matchId || !managerId || matchId !== `browser_${managerId}`) return null;
  return { matchId, managerId };
}

export function formatNodeId(id: string | null | undefined) {
  if (!id) return "None";
  if (id.length <= 8) return id;
  return `${id.substring(0, 4)}...${id.substring(id.length - 4)}`;
}
