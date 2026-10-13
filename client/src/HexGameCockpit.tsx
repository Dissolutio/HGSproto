import { useEffect, useMemo, useRef, useState, type CSSProperties, type RefObject } from "react";
import { Canvas, useFrame, useThree, type ThreeEvent } from "@react-three/fiber";
import { OrbitControls, Stats } from "@react-three/drei";
import * as THREE from "three";
import init, {
  wasm_get_current_state,
  wasm_initialize_match,
  wasm_load_mod_profiles,
  wasm_process_token_for_match,
  wasm_reset_match,
} from "./wasm/hex_game_core";

type Position = { q: number; r: number; z: number };
type EngineComponent = {
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
type EngineEntity = {
  id?: string;
  spatial?: Position;
  visual_primitive?: string;
  display_name?: string;
  components?: Record<string, EngineComponent> | EngineComponent[];
};
type EngineState = {
  context?: { match_id?: string; active_player?: string; turn_phase?: string };
  entities?: Record<string, EngineEntity> | EngineEntity[];
};
type RenderEntity = EngineEntity & { id: string; spatial: Position };
type TerrainData = RenderEntity & {
  faceTextureUrl: string;
  sideTextureUrl: string;
};
type Mode = "network" | "wasm";
type ConnectionState = "connecting" | "connected" | "offline" | "ready";
type ModProfileSummary = {
  modId: string;
  displayName: string;
  visualPrimitive: string;
};

const SOCKET_URL = "ws://127.0.0.1:8080";
const WASM_MATCH_ID = "wasm_sandbox";
const TAB_ID_STORAGE_KEY = "hexgamesim.tab-id";
const ACTIVE_MATCH_ID_STORAGE_KEY = "active_match_id";
const CACHED_MANAGER_ID_STORAGE_KEY = "cached_manager_id";
const HEX_RADIUS = 1;
const PILLAR_HEIGHT = 0.42;

type RecoverySession = { matchId: string; managerId: string };

function getTabId(): string {
  const existingTabId = window.sessionStorage.getItem(TAB_ID_STORAGE_KEY);
  if (existingTabId) return existingTabId;
  const tabId = `tab_${window.crypto.randomUUID().replace(/-/g, "")}`;
  window.sessionStorage.setItem(TAB_ID_STORAGE_KEY, tabId);
  return tabId;
}

function readRecoverySession(): RecoverySession | null {
  const matchId = window.sessionStorage.getItem(ACTIVE_MATCH_ID_STORAGE_KEY);
  const managerId = window.sessionStorage.getItem(CACHED_MANAGER_ID_STORAGE_KEY);
  if (!matchId || !managerId || matchId !== `browser_${managerId}`) return null;
  return { matchId, managerId };
}

const formatNodeId = (id: string | null | undefined) => {
  if (!id) return "None";
  if (id.length <= 8) return id;
  return `${id.substring(0, 4)}...${id.substring(id.length - 4)}`;
};

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

const MOCK_MAP_JSON = JSON.stringify({
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

function componentList(entity: EngineEntity): EngineComponent[] {
  return Array.isArray(entity.components)
    ? entity.components
    : Object.values(entity.components ?? {});
}

function findComponent(entity: EngineEntity, name: string): EngineComponent | undefined {
  return componentList(entity).find(
    (component) => component.type === name || name in component,
  );
}

function terrainTextures(entity: EngineEntity) {
  const terrain = findComponent(entity, "Terrain");
  return {
    faceTextureUrl:
      terrain?.data?.face_texture ?? terrain?.Terrain?.face_texture ?? "grass_face",
    sideTextureUrl:
      terrain?.data?.side_texture ?? terrain?.Terrain?.side_texture ?? "earth_side",
  };
}

function unitLabels(entity: EngineEntity): string[] {
  const tagged = findComponent(entity, "Tags");
  return tagged?.data?.labels ?? tagged?.Tags?.labels ?? [];
}

function unitHealth(entity: EngineEntity): { currentLife: number; maxLife: number } | null {
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

function entitiesFromState(state: EngineState | null): RenderEntity[] {
  if (!state?.entities) return [];
  const entries = Array.isArray(state.entities)
    ? state.entities.map((entity, index) => [entity.id ?? String(index), entity] as const)
    : Object.entries(state.entities);

  return entries.flatMap(([key, entity]) => {
    if (!entity?.spatial || !entity.components) return [];
    return [{ ...entity, id: entity.id ?? key, spatial: entity.spatial }];
  });
}

function parseState(serializedState: string): EngineState {
  const parsed = JSON.parse(serializedState) as EngineState | { error?: string };
  if ("error" in parsed && parsed.error) throw new Error(parsed.error);
  return parsed as EngineState;
}

function injectGoliath(entity: EngineEntity): EngineEntity {
  const components = entity.components;
  if (Array.isArray(components)) {
    let hasDestructible = false;
    const updatedComponents = components.map((component) => {
      if (component.type === "Destructible") {
        hasDestructible = true;
        return {
          ...component,
          data: { ...component.data, current_life: 12, max_life: 12 },
        };
      }
      if (component.Destructible) {
        hasDestructible = true;
        return {
          ...component,
          Destructible: { ...component.Destructible, current_life: 12, max_life: 12 },
        };
      }
      if (component.destructible) {
        hasDestructible = true;
        return {
          ...component,
          destructible: { ...component.destructible, current_life: 12, max_life: 12 },
        };
      }
      return component;
    });
    if (!hasDestructible) {
      updatedComponents.push({
        type: "Destructible",
        data: { current_life: 12, max_life: 12 },
      });
    }
    return {
      ...entity,
      visual_primitive: "CUBE",
      display_name: "Goliath Exo-Suit",
      components: updatedComponents,
    };
  }

  const existingDestructible = components?.destructible ?? {};
  return {
    ...entity,
    visual_primitive: "CUBE",
    display_name: "Goliath Exo-Suit",
    components: {
      ...components,
      destructible: {
        ...existingDestructible,
        type: existingDestructible.type ?? "Destructible",
        data: {
          ...existingDestructible.data,
          current_life: 12,
          max_life: 12,
        },
      },
    },
  };
}

function makeProceduralTexture(textureKey: string, isSide: boolean): THREE.CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = 128;
  canvas.height = 128;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D context is unavailable");

  const colorSets: Record<string, [string, string, string]> = {
    grass: ["#71834c", "#87965a", "#56683d"],
    earth: ["#66513e", "#806447", "#4d4033"],
    rock: ["#777d79", "#969b94", "#555c5a"],
    water: ["#487e89", "#64a1a4", "#315c6a"],
    sand: ["#c5aa70", "#ddc48a", "#998354"],
    default: ["#718079", "#8b9990", "#58665e"],
  };
  const paletteKey = textureKey.toLowerCase().split(/[_.-]/)[0];
  const palette = colorSets[paletteKey] ?? colorSets.default;
  context.fillStyle = palette[0];
  context.fillRect(0, 0, canvas.width, canvas.height);

  if (isSide) {
    context.fillStyle = palette[2];
    context.fillRect(0, 0, canvas.width, 12);
    context.globalAlpha = 0.3;
    for (let y = 22; y < canvas.height; y += 28) {
      context.fillStyle = y % 2 === 0 ? palette[1] : palette[2];
      context.fillRect(0, y, canvas.width, 3);
    }
    context.globalAlpha = 1;
  }

  let seed = 0;
  for (const character of textureKey) seed = (seed * 31 + character.charCodeAt(0)) >>> 0;
  for (let index = 0; index < 100; index += 1) {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    const x = seed % canvas.width;
    seed = (seed * 1664525 + 1013904223) >>> 0;
    const y = seed % canvas.height;
    const radius = 0.6 + ((seed >>> 8) % 18) / 10;
    context.globalAlpha = 0.12 + ((seed >>> 16) % 24) / 100;
    context.fillStyle = seed & 1 ? palette[1] : palette[2];
    context.beginPath();
    context.arc(x, y, radius, 0, Math.PI * 2);
    context.fill();
  }
  context.globalAlpha = 1;

  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.wrapS = THREE.RepeatWrapping;
  texture.wrapT = THREE.RepeatWrapping;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  return texture;
}

type ProceduralHexPillarProps = {
  q: number;
  r: number;
  z: number;
  faceTextureUrl: string;
  sideTextureUrl: string;
  hexRadius: number;
  onClick?: (event: ThreeEvent<MouseEvent>) => void;
};

function ProceduralHexPillar({
  q,
  r,
  z,
  faceTextureUrl,
  sideTextureUrl,
  hexRadius,
  onClick,
}: ProceduralHexPillarProps) {
  const sizeX = hexRadius * Math.sqrt(3);
  const sizeY = hexRadius * 1.5;
  const worldX = sizeX * (q + r * 0.5);
  const worldZ = sizeY * r;
  const worldY = z * 0.5;

  const geometry = useMemo(() => {
    const cylinder = new THREE.CylinderGeometry(
      hexRadius,
      hexRadius,
      PILLAR_HEIGHT,
      6,
      1,
      false,
    );
    // CylinderGeometry groups: sides=0, top=1, bottom=2. Use one cap material for both caps.
    for (const group of cylinder.groups) {
      if (group.materialIndex === 2) group.materialIndex = 1;
    }
    return cylinder;
  }, [hexRadius]);

  const sideTexture = useMemo(
    () => makeProceduralTexture(sideTextureUrl, true),
    [sideTextureUrl],
  );
  const faceTexture = useMemo(
    () => makeProceduralTexture(faceTextureUrl, false),
    [faceTextureUrl],
  );

  useEffect(() => () => {
    geometry.dispose();
    sideTexture.dispose();
    faceTexture.dispose();
  }, [geometry, sideTexture, faceTexture]);

  const sideMaterial = useMemo(
    () => new THREE.MeshStandardMaterial({ map: sideTexture, roughness: 0.94 }),
    [sideTexture],
  );
  const faceMaterial = useMemo(
    () => new THREE.MeshStandardMaterial({ map: faceTexture, roughness: 0.88 }),
    [faceTexture],
  );
  useEffect(() => () => {
    sideMaterial.dispose();
    faceMaterial.dispose();
  }, [sideMaterial, faceMaterial]);

  return (
    <mesh
      geometry={geometry}
      material={[sideMaterial, faceMaterial]}
      position={[worldX, worldY + PILLAR_HEIGHT / 2, worldZ]}
      castShadow
      receiveShadow
      onClick={onClick}
    />
  );
}

type UnitMarkerProps = {
  entity: RenderEntity;
  selected: boolean;
  onSelect: (event: ThreeEvent<MouseEvent>) => void;
};

function UnitMarker({ entity, selected, onSelect }: UnitMarkerProps) {
  const labels = unitLabels(entity);
  const playerOne = labels.includes("P1") || labels.includes("Player_1");
  const sizeX = HEX_RADIUS * Math.sqrt(3);
  const sizeY = HEX_RADIUS * 1.5;
  const worldX = sizeX * (entity.spatial.q + entity.spatial.r * 0.5);
  const worldZ = sizeY * entity.spatial.r;
  const worldY = entity.spatial.z * 0.5 + PILLAR_HEIGHT + 0.08;
  const visualPrimitive = entity.visual_primitive?.trim().toUpperCase();
  const isSphere = visualPrimitive === "SPHERE";
  const unitColor = playerOne ? "#3b82f6" : "#ef4444";

  return (
    <>
      <group position={[worldX, worldY, worldZ]} onClick={onSelect}>
        {selected && (
          <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.005, 0]}>
            <ringGeometry args={[0.3, 0.38, 32]} />
            <meshBasicMaterial color="#f0d47a" />
          </mesh>
        )}
        {isSphere ? (
          <mesh castShadow position={[0, 0.3, 0]}>
            <sphereGeometry args={[0.3, 16, 16]} />
            <meshStandardMaterial color={unitColor} />
          </mesh>
        ) : (
          <mesh castShadow position={[0, 0.3, 0]}>
            <boxGeometry args={[0.4, 0.4, 0.4]} />
            <meshStandardMaterial color={unitColor} />
          </mesh>
        )}
      </group>
    </>
  );
}

function UnitHealthProjection({
  units,
  target,
  labelElements,
}: {
  units: RenderEntity[];
  target: THREE.Vector3;
  labelElements: RefObject<Map<string, HTMLDivElement>>;
}) {
  const { camera, size } = useThree();
  const projectedUnits = useMemo(
    () =>
      units.map((entity) => {
        const sizeX = HEX_RADIUS * Math.sqrt(3);
        const sizeY = HEX_RADIUS * 1.5;
        const worldX = sizeX * (entity.spatial.q + entity.spatial.r * 0.5);
        const worldZ = sizeY * entity.spatial.r;
        const hero = entity.id.includes("HERO");
        const worldY =
          entity.spatial.z * 0.5 + PILLAR_HEIGHT + 0.08 + (hero ? 1.1 : 0.8);

        return {
          entity,
          health: unitHealth(entity),
          position: new THREE.Vector3(
            worldX - target.x,
            worldY - target.y,
            worldZ - target.z,
          ),
        };
      }),
    [units, target],
  );
  const projectedPosition = useMemo(() => new THREE.Vector3(), []);

  useFrame(() => {
    for (const { entity, position } of projectedUnits) {
      const element = labelElements.current.get(entity.id);
      if (!element) continue;

      projectedPosition.copy(position).project(camera);
      const visible =
        projectedPosition.z >= -1 &&
        projectedPosition.z <= 1 &&
        Math.abs(projectedPosition.x) <= 1 &&
        Math.abs(projectedPosition.y) <= 1;
      element.style.visibility = visible ? "visible" : "hidden";
      if (visible) {
        const x = (projectedPosition.x + 1) * size.width / 2;
        const y = (1 - projectedPosition.y) * size.height / 2;
        element.style.transform =
          `translate3d(${x}px, ${y}px, 0) translate(-50%, -50%)`;
      }
    }
  });

  return (
    null
  );
}

type SceneProps = {
  terrain: TerrainData[];
  units: RenderEntity[];
  selectedUnitId: string | null;
  healthLabelElements: RefObject<Map<string, HTMLDivElement>>;
  onSelectUnit: (entity: RenderEntity, event: ThreeEvent<MouseEvent>) => void;
  onHexClick: (entity: TerrainData) => void;
};

function HexScene({
  terrain,
  units,
  selectedUnitId,
  healthLabelElements,
  onSelectUnit,
  onHexClick,
}: SceneProps) {
  const target = useMemo(() => {
    if (terrain.length === 0) return new THREE.Vector3(0, 0, 0);
    const sum = terrain.reduce(
      (accumulator, cell) => {
        accumulator.x += HEX_RADIUS * Math.sqrt(3) * (cell.spatial.q + cell.spatial.r * 0.5);
        accumulator.y += cell.spatial.z * 0.5;
        accumulator.z += HEX_RADIUS * 1.5 * cell.spatial.r;
        return accumulator;
      },
      new THREE.Vector3(),
    );
    return sum.multiplyScalar(1 / terrain.length);
  }, [terrain]);

  return (
    <>
      <color attach="background" args={["#101c1d"]} />
      <ambientLight intensity={0.6} />
      <directionalLight position={[10, 20, 10]} intensity={1.2} castShadow />
      <group position={[-target.x, -target.y, -target.z]}>
        {terrain.map((cell) => (
          <ProceduralHexPillar
            key={cell.id}
            q={cell.spatial.q}
            r={cell.spatial.r}
            z={cell.spatial.z}
            faceTextureUrl={cell.faceTextureUrl}
            sideTextureUrl={cell.sideTextureUrl}
            hexRadius={HEX_RADIUS}
            onClick={(event) => {
              event.stopPropagation();
              onHexClick(cell);
            }}
          />
        ))}
        {units.map((unit) => (
          <UnitMarker
            key={unit.id}
            entity={unit}
            selected={selectedUnitId === unit.id}
            onSelect={(event) => onSelectUnit(unit, event)}
          />
        ))}
      </group>
      <UnitHealthProjection
        units={units}
        target={target}
        labelElements={healthLabelElements}
      />
      <OrbitControls makeDefault target={[0, 0, 0]} enableDamping dampingFactor={0.08} />
      <Stats />
    </>
  );
}

export default function HexGameCockpit() {
  const [tabId] = useState(getTabId);
  const [mode, setMode] = useState<Mode>("wasm");
  const [gameState, setGameState] = useState<EngineState | null>(null);
  const [selectedUnitId, setSelectedUnitId] = useState<string | null>(null);
  const [inspectedEntity, setInspectedEntity] = useState<RenderEntity | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [notice, setNotice] = useState("Loading local WebAssembly engine");
  const [socket, setSocket] = useState<WebSocket | null>(null);
  const [modProfileText, setModProfileText] = useState("");
  const [loadedModProfiles, setLoadedModProfiles] = useState<ModProfileSummary[]>([]);
  const [localNodeId, setLocalNodeId] = useState<string | null>(null);
  const [localMatchId, setLocalMatchId] = useState<string | null>(null);
  const [remoteNodeId, setRemoteNodeId] = useState<string | null>(null);
  const [isPeerConnected, setIsPeerConnected] = useState<boolean>(false);
  const [joinNodeId, setJoinNodeId] = useState("");
  const [recoverySession, setRecoverySession] = useState<RecoverySession | null>(
    readRecoverySession,
  );
  const [recoveryDismissed, setRecoveryDismissed] = useState(false);
  const [connectionAttempt, setConnectionAttempt] = useState(0);
  const reconnectAttempts = useRef(0);
  const healthLabelElements = useRef(new Map<string, HTMLDivElement>());

  useEffect(() => {
    let cancelled = false;
    let ws: WebSocket | null = null;
    let retryTimer: number | undefined;

    if (mode === "wasm") {
      setSocket(null);
      setConnection("connecting");
      setIsPeerConnected(false);
      setNotice("Loading local WebAssembly engine");
      void (async () => {
        try {
          await init();
          if (cancelled) return;
          wasm_initialize_match(WASM_MATCH_ID, MOCK_MAP_JSON);
          const state = parseState(wasm_get_current_state(WASM_MATCH_ID));
          if (cancelled) return;
          setGameState(state);
          setConnection("ready");
          setNotice("Autonomous WASM sandbox ready");
        } catch (error) {
          if (cancelled) return;
          setConnection("offline");
          setNotice(`WASM initialization failed: ${error instanceof Error ? error.message : String(error)}`);
        }
      })();
      return () => {
        cancelled = true;
      };
    }

    setConnection("connecting");
    const savedSession = readRecoverySession();
    const managerId =
      savedSession?.managerId ??
      window.sessionStorage.getItem(CACHED_MANAGER_ID_STORAGE_KEY) ??
      `${tabId}_${window.crypto.randomUUID().slice(0, 8)}`;
    window.sessionStorage.setItem(CACHED_MANAGER_ID_STORAGE_KEY, managerId);
    let activeMatchId = savedSession?.matchId ?? null;
    let recoveredActionCount = 0;

    const persistActiveSession = (matchId: string) => {
      activeMatchId = matchId;
      window.sessionStorage.setItem(ACTIVE_MATCH_ID_STORAGE_KEY, matchId);
      window.sessionStorage.setItem(CACHED_MANAGER_ID_STORAGE_KEY, managerId);
      setRecoverySession({ matchId, managerId });
      setRecoveryDismissed(false);
    };
    const clearActiveSession = () => {
      activeMatchId = null;
      window.sessionStorage.removeItem(ACTIVE_MATCH_ID_STORAGE_KEY);
      window.sessionStorage.removeItem(CACHED_MANAGER_ID_STORAGE_KEY);
      setRecoverySession(null);
    };
    const requestInitialMapSnapshotFromCore = () => {
      if (!ws || ws.readyState !== WebSocket.OPEN) {
        setNotice("Host key allocated, but the engine stream is not ready for a map snapshot");
        return;
      }
      try {
        ws.send("GET_STATE");
        setNotice("Host key allocated; requesting the initial map snapshot");
      } catch (error) {
        setNotice(`Initial map snapshot request failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    };
    const connect = () => {
      if (cancelled) return;
      ws = new WebSocket(SOCKET_URL);
      const gatewaySocket = ws;
      setSocket(gatewaySocket);
      gatewaySocket.onopen = () => {
        if (cancelled) return;
        try {
          gatewaySocket.send(
            activeMatchId
              ? `RECONNECT_SESSION:${activeMatchId}:${managerId}`
              : `SESSION_ID:${managerId}:${MOCK_MAP_JSON}`,
          );
          setConnection("connected");
          setNotice(
            activeMatchId
              ? "Reconnecting to the existing match session…"
              : "Live engine stream connected",
          );
        } catch (error) {
          setNotice(`Network session handshake failed: ${error instanceof Error ? error.message : String(error)}`);
          gatewaySocket.close();
        }
      };
      gatewaySocket.onmessage = (event) => {
        if (cancelled) return;
        const message = String(event.data);
        if (message === "PEER_DISCONNECTED" || message.startsWith("PEER_DISCONNECTED:")) {
          setIsPeerConnected(false);
          return;
        }
        if (
          message === "PEER_CONNECTED" ||
          message.startsWith("PEER_CONNECTED:") ||
          message === "PEER_RECOVERED" ||
          message.startsWith("PEER_RECOVERED:")
        ) {
          setIsPeerConnected(true);
          return;
        }
        if (message.startsWith("HOST_ALLOCATED:") || message.startsWith("P2P_HOSTED:")) {
          const prefix = message.startsWith("HOST_ALLOCATED:")
            ? "HOST_ALLOCATED:"
            : "P2P_HOSTED:";
          const [allocatedId, matchId] = message.slice(prefix.length).split(":", 2);
          if (!allocatedId || (prefix === "HOST_ALLOCATED:" && !matchId)) {
            setNotice("Host allocation response did not include a valid connection ticket");
            return;
          }
          setLocalNodeId(allocatedId);
          setLocalMatchId(matchId || null);
          if (matchId) persistActiveSession(matchId);
          setRemoteNodeId(null);
          setIsPeerConnected(false);
          requestInitialMapSnapshotFromCore();
          return;
        }
        if (message.startsWith("P2P_JOINED:")) {
          const [, localId, remoteId] = message.split(":", 3);
          setLocalNodeId(localId || null);
          setRemoteNodeId(remoteId || null);
          setIsPeerConnected(!!remoteId);
          setLocalMatchId(null);
          persistActiveSession(`browser_${managerId}`);
          setNotice("Joined and synchronized with the host");
          return;
        }
        if (message.startsWith("P2P_CONNECTED:")) {
          setRemoteNodeId(message.slice("P2P_CONNECTED:".length) || null);
          setIsPeerConnected(true);
          setNotice("Peer connected; action-token synchronization is active");
          return;
        }
        if (message.startsWith("P2P_DISCONNECTED:")) {
          setRemoteNodeId(null);
          setIsPeerConnected(false);
          setNotice(`Peer disconnected: ${message.slice("P2P_DISCONNECTED:".length)}`);
          return;
        }
        if (message.startsWith("ACTION_LOG:")) {
          try {
            const actionLog: unknown = JSON.parse(message.slice("ACTION_LOG:".length));
            if (!Array.isArray(actionLog) || !actionLog.every((action) => typeof action === "string")) {
              throw new Error("Gateway returned an invalid action log");
            }
            recoveredActionCount = actionLog.length;
          } catch (error) {
            setNotice(`Could not restore action history: ${error instanceof Error ? error.message : String(error)}`);
          }
          return;
        }
        if (message.startsWith("P2P_RECONNECTED:")) {
          const [, matchId, localId, remoteId, role] = message.split(":", 5);
          if (matchId) persistActiveSession(matchId);
          setLocalNodeId(localId || null);
          setRemoteNodeId(remoteId || null);
          setIsPeerConnected(!!remoteId);
          setLocalMatchId(role === "HOST" ? matchId || null : null);
          setConnection("connected");
          setRecoverySession(null);
          reconnectAttempts.current = 0;
          setNotice(
            `Reconnected; restored match state and ${recoveredActionCount} recorded action${recoveredActionCount === 1 ? "" : "s"}`,
          );
          return;
        }
        if (message === "P2P_RESET") {
          clearActiveSession();
          setLocalNodeId(null);
          setLocalMatchId(null);
          setRemoteNodeId(null);
          setIsPeerConnected(false);
          setJoinNodeId("");
          setSelectedUnitId(null);
          setInspectedEntity(null);
          triggerFreshMapSnapshotLoad(false);
          return;
        }
        if (message === "E:SESSION_EXPIRED") {
          clearActiveSession();
          setLocalNodeId(null);
          setLocalMatchId(null);
          setRemoteNodeId(null);
          setIsPeerConnected(false);
          setNotice("The previous match session expired; reconnecting with a fresh session");
          gatewaySocket.close();
          return;
        }
        if (message.startsWith("E:")) {
          setNotice(message.slice(2));
          return;
        }
        try {
          setGameState(parseState(message));
          setNotice("State synchronized");
        } catch (error) {
          setNotice(`Invalid engine response: ${error instanceof Error ? error.message : String(error)}`);
        }
      };
      gatewaySocket.onerror = () => {
        if (!cancelled) {
          setIsPeerConnected(false);
          setNotice("WebSocket error; retaining session details and retrying");
        }
      };
      gatewaySocket.onclose = () => {
        if (cancelled) return;
        setConnection("offline");
        setIsPeerConnected(false);
        const storedSession = readRecoverySession();
        if (storedSession) setRecoverySession(storedSession);
        setNotice(
          storedSession
            ? "Unexpected disconnect; preserving match details and retrying…"
            : "Engine stream disconnected; retrying…",
        );
        const delay = Math.min(30_000, 1_000 * 2 ** Math.min(reconnectAttempts.current, 5));
        reconnectAttempts.current += 1;
        retryTimer = window.setTimeout(() => {
          setConnectionAttempt((attempt) => attempt + 1);
        }, delay);
      };
    };

    if (savedSession) {
      setRecoverySession(savedSession);
      setNotice("Restoring the previous network match session…");
    } else {
      setNotice("Opening local engine stream");
    }
    const handleWindowClose = () => {
      const activeSocket = ws;
      if (!activeSocket || activeSocket.readyState !== WebSocket.OPEN) return;
      activeSocket.close();
    };
    window.addEventListener("beforeunload", handleWindowClose);
    connect();
    return () => {
      window.removeEventListener("beforeunload", handleWindowClose);
      cancelled = true;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
      ws?.close();
      setSocket((activeSocket) => activeSocket === ws ? null : activeSocket);
    };
  }, [mode, tabId, connectionAttempt]);

  const entities = useMemo(() => entitiesFromState(gameState), [gameState]);
  const terrain = useMemo(
    () => entities.flatMap((entity): TerrainData[] => {
      const terrainComponent = findComponent(entity, "Terrain");
      if (!terrainComponent) return [];
      const textures = terrainTextures(entity);
      return [{ ...entity, ...textures }];
    }),
    [entities],
  );
  const units = useMemo(
    () => entities.filter((entity) => unitLabels(entity).includes("Unit")),
    [entities],
  );

  const findEntityAtCoordinate = (q: number, r: number, z: number) =>
    units.find(
      (unit) =>
        unit.spatial.q === q &&
        unit.spatial.r === r &&
        unit.spatial.z === z,
    );

  const sync3DCanvasWithWasmState = () => {
    setGameState(parseState(wasm_get_current_state(WASM_MATCH_ID)));
  };

  const handleHexClick = (cell: TerrainData) => {
    setInspectedEntity(entities.find((entity) => entity.id === cell.id) ?? cell);
    const { q, r, z } = cell.spatial;
    if (!selectedUnitId) {
      const occupant = findEntityAtCoordinate(q, r, z);
      if (occupant) {
        setSelectedUnitId(occupant.id);
        setNotice(`${occupant.id} selected`);
      }
      return;
    }

    const occupied = units.some(
      (unit) =>
        unit.id !== selectedUnitId &&
        unit.spatial.q === q &&
        unit.spatial.r === r &&
        unit.spatial.z === z,
    );
    if (occupied) {
      setNotice("Move rejected locally: that hex is occupied");
      return;
    }

    const token = `M:${selectedUnitId}:Q${q}R${r}Z${z}`;
    if (mode === "wasm") {
      try {
        const presentationEcho = wasm_process_token_for_match(WASM_MATCH_ID, token);
        if (presentationEcho.startsWith("E:")) {
          console.error("Referee Rejected Action:", presentationEcho);
          setNotice(presentationEcho.slice(2));
          return;
        }
        setSelectedUnitId(null);
        sync3DCanvasWithWasmState();
        setNotice(`Applied locally: ${token}`);
      } catch (error) {
        setNotice(`WASM move failed: ${error instanceof Error ? error.message : String(error)}`);
      }
      return;
    }
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      setNotice("Move not sent: engine stream is offline");
      return;
    }
    try {
      socket.send(token);
      setNotice(`Sent ${token}`);
      setSelectedUnitId(null);
    } catch (error) {
      setNotice(`Move could not be sent: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const selectUnit = (unit: RenderEntity, event: ThreeEvent<MouseEvent>) => {
    event.stopPropagation();
    setInspectedEntity(unit);
    const nextId = selectedUnitId === unit.id ? null : unit.id;
    setSelectedUnitId(nextId);
    setNotice(nextId ? `${unit.id} selected` : "Selection cleared");
  };

  const chooseMode = (nextMode: Mode) => {
    if (nextMode === mode) return;
    setSelectedUnitId(null);
    setInspectedEntity(null);
    setGameState(null);
    setMode(nextMode);
  };

  const loadModProfiles = () => {
    if (!modProfileText.trim()) {
      setNotice("Paste a mod profile before loading");
      return;
    }
    try {
      const parsed: unknown = JSON.parse(modProfileText);
      const profiles = Array.isArray(parsed) ? parsed : [parsed];
      const summaries = profiles.map((profile): ModProfileSummary => {
        if (!profile || typeof profile !== "object" || Array.isArray(profile)) {
          throw new Error("Each mod profile must be a JSON object");
        }
        const record = profile as Record<string, unknown>;
        return {
          modId: typeof record.mod_id === "string" ? record.mod_id : "Unknown mod ID",
          displayName:
            typeof record.display_name === "string" ? record.display_name : "Unnamed profile",
          visualPrimitive:
            typeof record.visual_primitive === "string"
              ? record.visual_primitive
              : "Unknown primitive",
        };
      });
      const loadedCount = wasm_load_mod_profiles(modProfileText);
      setLoadedModProfiles(summaries);
      setNotice(
        `Loaded ${loadedCount} community mod profile${loadedCount === 1 ? "" : "s"}`,
      );
    } catch (error) {
      setNotice(
        `Mod profile load failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  };

  const handleDevInjectGoliath = () => {
    if (!selectedUnitId) {
      window.alert("Please click to select a 3D unit mesh on the board first!");
      return;
    }
    const selectedUnit = units.find((unit) => unit.id === selectedUnitId);
    if (!selectedUnit) {
      setNotice(`Cannot inject Goliath: selected unit '${selectedUnitId}' is no longer available`);
      return;
    }

    setGameState((previous) => {
      if (!previous?.entities) return previous;
      if (Array.isArray(previous.entities)) {
        return {
          ...previous,
          entities: previous.entities.map((entity, index) =>
            (entity.id ?? String(index)) === selectedUnitId
              ? injectGoliath(entity)
              : entity,
          ),
        };
      }
      return {
        ...previous,
        entities: Object.fromEntries(
          Object.entries(previous.entities).map(([key, entity]) => [
            key,
            (entity.id ?? key) === selectedUnitId ? injectGoliath(entity) : entity,
          ]),
        ),
      };
    });
    setInspectedEntity({
      ...injectGoliath(selectedUnit),
      id: selectedUnit.id,
      spatial: selectedUnit.spatial,
    });
    setNotice(`Injected Goliath Exo-Suit onto ${selectedUnitId}`);
  };

  const sendNetworkControl = (message: string) => {
    if (mode !== "network" || !socket || socket.readyState !== WebSocket.OPEN) {
      setNotice("Switch to Live Network Mode and connect to the local Iroh gateway first");
      return;
    }
    try {
      socket.send(message);
      setNotice(message === "HOST" ? "Starting Iroh host endpoint…" : "Connecting to Iroh host…");
    } catch (error) {
      setNotice(`Network request failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const handleHostMatch = () => sendNetworkControl("HOST");

  const handleJoinMatch = () => {
    const ticket = joinNodeId.trim();
    const [hostNodeId, hostMatchId, extra] = ticket.split("@");
    if (!hostNodeId || !hostMatchId || extra !== undefined) {
      setNotice("Paste the complete host connection ticket (Node ID@match ID)");
      return;
    }
    sendNetworkControl(`JOIN:${hostNodeId}@${hostMatchId}`);
  };

  const handleGracefulReconnection = () => {
    const cachedSession = readRecoverySession();
    if (!cachedSession) {
      setRecoverySession(null);
      setNotice("No recoverable match session is available in this tab");
      return;
    }
    setRecoverySession(cachedSession);
    setRecoveryDismissed(false);
    setNotice(`Rebinding to match ${cachedSession.matchId}…`);
    if (mode !== "network") {
      setMode("network");
      return;
    }
    if (socket?.readyState === WebSocket.OPEN) {
      try {
        socket.send(
          `RECONNECT_SESSION:${cachedSession.matchId}:${cachedSession.managerId}`,
        );
        return;
      } catch (error) {
        setNotice(`Graceful reconnection failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
    setConnectionAttempt((attempt) => attempt + 1);
  };

  const triggerFreshMapSnapshotLoad = (gatewayResetRequested: boolean) => {
    if (mode === "network") {
      if (gatewayResetRequested) {
        setGameState(null);
        setNotice("Gateway reset requested; loading a fresh sandbox snapshot");
        return;
      }
    }
    void (async () => {
      try {
        await init();
        wasm_reset_match(WASM_MATCH_ID, MOCK_MAP_JSON);
        setGameState(parseState(wasm_get_current_state(WASM_MATCH_ID)));
        if (mode === "network") {
          setMode("wasm");
          setNotice("Native gateway unavailable; local WASM sandbox refreshed");
        } else {
          setNotice("Fresh sandbox map loaded");
        }
        setConnection("ready");
      } catch (error) {
        setConnection("offline");
        setNotice(`Sandbox reset failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    })();
  };

  const handleDisconnectAndReset = () => {
    let gatewayResetRequested = false;
    if (socket && socket.readyState === WebSocket.OPEN) {
      try {
        socket.send("HARD_RESET_SESSION");
        gatewayResetRequested = true;
      } catch (error) {
        setNotice(`Gateway reset request failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
    window.sessionStorage.removeItem(ACTIVE_MATCH_ID_STORAGE_KEY);
    window.sessionStorage.removeItem(CACHED_MANAGER_ID_STORAGE_KEY);
    setRecoverySession(null);
    setRecoveryDismissed(false);
    setLocalNodeId(null);
    setLocalMatchId(null);
    setRemoteNodeId(null);
    setIsPeerConnected(false);
    setJoinNodeId("");
    setSelectedUnitId(null);
    setInspectedEntity(null);
    triggerFreshMapSnapshotLoad(gatewayResetRequested);
  };

  const playerOneCount = units.filter((unit) => {
    const labels = unitLabels(unit);
    return labels.includes("P1") || labels.includes("Player_1");
  }).length;
  const playerTwoCount = units.length - playerOneCount;
  const storedRecoverySession = recoverySession ? readRecoverySession() : null;
  const showRecoveryBanner =
    mode === "network" &&
    !recoveryDismissed &&
    !!storedRecoverySession &&
    storedRecoverySession.matchId === recoverySession?.matchId &&
    socket?.readyState === WebSocket.CLOSED;

  return (
    <main style={styles.page}>
      <nav style={styles.modeBar} aria-label="Simulation mode">
        <button
          type="button"
          aria-pressed={mode === "network"}
          onClick={() => chooseMode("network")}
          style={{ ...styles.modeButton, ...(mode === "network" ? styles.modeButtonActive : {}) }}
        >
          Live Network Mode (WebSocket){" "}
          <span style={styles.modeNote}>
            (Multiplayer Portal — Active Native Server Required)
          </span>
        </button>
        <button
          type="button"
          aria-pressed={mode === "wasm"}
          onClick={() => chooseMode("wasm")}
          style={{ ...styles.modeButton, ...(mode === "wasm" ? styles.modeButtonActive : {}) }}
        >
          Autonomous Sandbox Mode (WASM)
        </button>
      </nav>

      <header style={styles.header}>
        <div>
          <p style={styles.kicker}>HEXGAMESIM / {mode === "network" ? "LIVE SESSION" : "LOCAL SIMULATION"}</p>
          <h1 style={styles.title}>Tactical board</h1>
        </div>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 18, flexWrap: "wrap" }}>
          <div style={styles.connection}>
            <span
              style={{
                ...styles.statusDot,
                background:
                  connection === "connected" || connection === "ready"
                    ? "#70d6a4"
                    : connection === "connecting"
                      ? "#edbd62"
                      : "#ef7777",
              }}
            />
            <span>{connection}</span>
            <span style={styles.matchId}>
              {gameState?.context?.match_id ?? (mode === "wasm" ? WASM_MATCH_ID : "waiting for state")}
            </span>
          </div>
          <div
            style={{ ...styles.connection, textTransform: "none" }}
            role="status"
            aria-live="polite"
            aria-label={isPeerConnected ? "Peer connected" : "Peer disconnected"}
          >
            <span>👤 Peer Status:</span>
            {isPeerConnected && (
              <style>
                {`@keyframes peer-connection-pulse {
                  0%, 100% { box-shadow: 0 0 0 0 rgba(34, 197, 94, 0.65); }
                  50% { box-shadow: 0 0 0 5px rgba(34, 197, 94, 0); }
                }`}
              </style>
            )}
            <span
              style={{
                ...styles.statusDot,
                background: isPeerConnected ? "#22c55e" : "#ef4444",
                animation: isPeerConnected
                  ? "peer-connection-pulse 1.4s ease-in-out infinite"
                  : undefined,
              }}
            />
            <span>{isPeerConnected ? "Connected" : "(Disconnected)"}</span>
          </div>
        </div>
      </header>

      {showRecoveryBanner && recoverySession && (
        <aside
          role="alert"
          style={{
            margin: "12px 0",
            padding: "12px 16px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            border: "1px solid #b45309",
            borderRadius: 8,
            background: "#fff7ed",
            color: "#7c2d12",
            fontWeight: 700,
          }}
        >
          <span>⚠️ Unexpected Disconnect Detected from Match {recoverySession.matchId}</span>
          <button
            type="button"
            onClick={handleGracefulReconnection}
            style={{
              padding: "8px 12px",
              border: 0,
              borderRadius: 6,
              background: "#b45309",
              color: "#fff",
              cursor: "pointer",
              fontWeight: 700,
            }}
          >
            🔌 Attempt Graceful Reconnection
          </button>
          <button
            type="button"
            aria-label="Dismiss disconnect notification"
            onClick={() => setRecoveryDismissed(true)}
            style={{
              padding: "4px 8px",
              border: 0,
              borderRadius: 6,
              background: "transparent",
              color: "#7c2d12",
              cursor: "pointer",
              fontSize: 18,
              fontWeight: 700,
            }}
          >
            ×
          </button>
        </aside>
      )}

      <section style={styles.toolbar} aria-live="polite">
        <span style={styles.notice}>{notice}</span>
        <span style={styles.counts}>{terrain.length} hexes <i /> {units.length} units</span>
      </section>

      <section style={styles.boardFrame} aria-label="3D tactical board">
        <Canvas
          camera={{ position: [8, 9, 11], fov: 42, near: 0.1, far: 500 }}
          shadows="percentage"
          frameloop="demand"
          dpr={[1, 1.75]}
          gl={{ antialias: true, powerPreference: "high-performance" }}
          style={styles.canvas}
        >
          <HexScene
            terrain={terrain}
            units={units}
            selectedUnitId={selectedUnitId}
            healthLabelElements={healthLabelElements}
            onSelectUnit={selectUnit}
            onHexClick={handleHexClick}
          />
        </Canvas>
        <div style={styles.healthOverlayLayer} aria-label="Unit health overlays">
          {units.map((unit) => {
            const health = unitHealth(unit);
            return (
              <div
                key={unit.id}
                ref={(element) => {
                  if (element) healthLabelElements.current.set(unit.id, element);
                  else healthLabelElements.current.delete(unit.id);
                }}
                data-unit-id={unit.id}
                style={styles.healthTag}
              >
                {health ? `${health.currentLife} / ${health.maxLife} HP` : "HP unavailable"}
              </div>
            );
          })}
        </div>
        <aside style={styles.inspector} aria-label="Entity component inspector">
          <h2 style={styles.inspectorTitle}>📊 HGS Real-Time Component Inspector</h2>
          {inspectedEntity ? (
            <pre style={styles.inspectorContent}>
              {JSON.stringify(inspectedEntity, null, 2)}
            </pre>
          ) : (
            <p style={styles.inspectorEmpty}>
              Click a unit or hex pillar to inspect its components.
            </p>
          )}
          <details style={styles.modLoader}>
            <summary style={styles.modLoaderSummary}>🔌 Community Mod Profile Loader</summary>
            <label style={styles.modLoaderLabel} htmlFor="mod-profile-json">
              Paste a mod profile or profile batch as JSON
            </label>
            <textarea
              id="mod-profile-json"
              aria-label="Community mod profile JSON"
              value={modProfileText}
              onChange={(event) => setModProfileText(event.target.value)}
              placeholder={'{\n  "schema_version": 1,\n  "mod_id": "com.community.scout",\n  "display_name": "Community Scout",\n  "visual_primitive": "CUBE",\n  "components": {\n    "destructible": { "current_life": 5, "max_life": 5 },\n    "tags": ["Unit"]\n  }\n}'}
              style={styles.modProfileInput}
            />
            <button
              type="button"
              onClick={loadModProfiles}
              disabled={mode !== "wasm" || connection !== "ready"}
              style={styles.modProfileButton}
            >
              Load Mod Profile
            </button>
            {import.meta.env.DEV && (
              <>
                <p role="note" style={{ margin: "8px 0 0", color: "#92400e", fontSize: 12 }}>
                  Developer presentation demo only: changes the React view, not authoritative
                  Rust/WASM or network match state.
                </p>
                <button
                  type="button"
                  onClick={handleDevInjectGoliath}
                  style={{
                    marginTop: 8,
                    padding: "4px 8px",
                    background: "#d97706",
                    color: "#fff",
                    border: "none",
                    borderRadius: 4,
                    cursor: "pointer",
                  }}
                >
                  🧪 Demo: show Goliath on selected unit
                </button>
              </>
            )}
            {loadedModProfiles.length > 0 && (
              <ul style={styles.modProfileList} aria-label="Loaded mod profiles">
                {loadedModProfiles.map((profile) => (
                  <li key={profile.modId}>
                    {profile.displayName} · {profile.modId} · {profile.visualPrimitive}
                  </li>
                ))}
              </ul>
            )}
          </details>
          <details style={styles.networkPortal}>
            <summary style={styles.modLoaderSummary}>🌐 Multiplayer Connection Portal</summary>
            <p style={styles.networkPortalHelp}>
              Host or join through the local native gateway. Iroh handles encrypted peer transport and relay discovery; browser traffic reaches it over this local WebSocket.
            </p>
            <table style={styles.networkStatusTable}>
              <tbody>
                <tr>
                  <th style={styles.networkStatusLabel}>Your Node ID:</th>
                  <td style={styles.networkStatusValue}>
                    {localNodeId
                      ? formatNodeId(localNodeId)
                      : <>None <em>(Click Host or Join to allocate key)</em></>}
                  </td>
                </tr>
                <tr>
                  <th style={styles.networkStatusLabel}>Opponent Node ID:</th>
                  <td style={styles.networkStatusValue}>
                    {remoteNodeId ? formatNodeId(remoteNodeId) : "Waiting for peer..."}
                  </td>
                </tr>
                <tr>
                  <th style={styles.networkStatusLabel}>Active Connection Transport:</th>
                  <td style={styles.networkStatusValue}>
                    {mode === "network" && connection === "connected"
                      ? "Local Gateway WebSocket (Port 8080)"
                      : "WASM Sandbox"}
                  </td>
                </tr>
              </tbody>
            </table>
            <button
              type="button"
              onClick={handleHostMatch}
              disabled={mode !== "network" || !socket || socket.readyState !== WebSocket.OPEN || !!localNodeId}
              style={styles.networkButton}
            >
              Host Match
            </button>
            {localNodeId && localMatchId && (
              <label style={styles.modLoaderLabel} htmlFor="local-iroh-node-id">
                Your connection ticket (copy for the other tab)
                <input
                  id="local-iroh-node-id"
                  readOnly
                  value={`${localNodeId}@${localMatchId}`}
                  onFocus={(event) => event.currentTarget.select()}
                  style={styles.networkInput}
                />
              </label>
            )}
            <label style={styles.modLoaderLabel} htmlFor="remote-iroh-node-id">
              Host connection ticket
              <input
                id="remote-iroh-node-id"
                value={joinNodeId}
                onChange={(event) => setJoinNodeId(event.target.value)}
                placeholder="Node ID@match ID"
                style={styles.networkInput}
              />
            </label>
            <button
              type="button"
              onClick={handleJoinMatch}
              disabled={mode !== "network" || !socket || socket.readyState !== WebSocket.OPEN}
              style={styles.networkButton}
            >
              Join Match
            </button>
            <button
              type="button"
              onClick={handleDisconnectAndReset}
              style={{
                marginTop: 12,
                width: "100%",
                padding: 6,
                background: "#dc2626",
                color: "#fff",
                border: "none",
                borderRadius: 4,
                cursor: "pointer",
                fontWeight: "bold",
              }}
            >
              🛑 Leave Match &amp; Reset Gateway
            </button>
          </details>
        </aside>
      </section>

      <footer style={styles.footer}>
        <span><b style={{ color: "#58a9e8" }}>P1</b> Player 1 · {playerOneCount}</span>
        <span><b style={{ color: "#f07868" }}>P2</b> Player 2 · {playerTwoCount}</span>
        <span style={styles.hint}>
          {selectedUnitId
            ? `Selected: ${selectedUnitId} · click an open pillar to move`
            : "Select a unit to issue a move"}
        </span>
      </footer>
    </main>
  );
}

const styles: Record<string, CSSProperties> = {
  page: {
    minHeight: "100vh",
    padding: "20px clamp(16px, 4vw, 56px) 28px",
    background: "#e9eee8",
    color: "#18302d",
    fontFamily: "'Avenir Next', Avenir, 'Segoe UI', sans-serif",
    boxSizing: "border-box",
  },
  modeBar: {
    display: "flex",
    justifyContent: "center",
    gap: 4,
    width: "fit-content",
    maxWidth: "100%",
    margin: "0 auto 24px",
    padding: 4,
    border: "1px solid #c4d0c7",
    background: "#dfe7df",
    flexWrap: "wrap",
  },
  modeButton: {
    borderWidth: 1,
    borderStyle: "solid",
    borderColor: "transparent",
    background: "transparent",
    color: "#49635a",
    padding: "10px 14px",
    font: "inherit",
    fontSize: 13,
    fontWeight: 600,
    cursor: "pointer",
  },
  modeNote: { color: "#78847d", fontSize: 10, fontWeight: 400 },
  modeButtonActive: {
    background: "#183b35",
    color: "#f1f4ed",
    borderColor: "#183b35",
  },
  header: {
    display: "flex",
    alignItems: "end",
    justifyContent: "space-between",
    gap: 20,
    maxWidth: 1240,
    margin: "0 auto",
    borderBottom: "1px solid #c4d0c7",
    paddingBottom: 18,
  },
  kicker: { margin: "0 0 8px", color: "#59736a", fontSize: 11, fontWeight: 700, letterSpacing: 1.5 },
  title: { margin: 0, fontSize: 28, fontWeight: 650, lineHeight: 1.1 },
  connection: { display: "flex", alignItems: "center", gap: 9, color: "#3f5a51", fontSize: 13, textTransform: "capitalize" },
  statusDot: { width: 9, height: 9, borderRadius: "50%", display: "inline-block" },
  matchId: { borderLeft: "1px solid #bcc9bf", paddingLeft: 12, fontFamily: "ui-monospace, monospace", color: "#75847c" },
  toolbar: { maxWidth: 1240, margin: "0 auto", minHeight: 52, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 },
  notice: { color: "#416158", fontSize: 13, overflowWrap: "anywhere" },
  counts: { color: "#587168", fontSize: 12, whiteSpace: "nowrap" },
  boardFrame: {
    position: "relative",
    maxWidth: 1240,
    height: "min(70vh, 760px)",
    minHeight: 400,
    margin: "0 auto",
    border: "1px solid #344d47",
    background: "#101c1d",
    boxShadow: "0 16px 38px #18302d20",
    overflow: "hidden",
  },
  inspector: {
    position: "absolute",
    zIndex: 2,
    top: 12,
    right: 12,
    width: "min(340px, calc(100% - 24px))",
    maxHeight: "calc(100% - 24px)",
    overflowY: "auto",
    boxSizing: "border-box",
    padding: 14,
    border: "1px solid #4d6860",
    background: "rgba(8, 20, 18, 0.86)",
    color: "#e5efe8",
    boxShadow: "0 8px 24px #0006",
    backdropFilter: "blur(6px)",
  },
  inspectorTitle: {
    margin: "0 0 10px",
    fontSize: 13,
    fontWeight: 650,
    lineHeight: 1.4,
  },
  inspectorContent: {
    margin: 0,
    fontSize: 11,
    fontFamily: "monospace",
    color: "#10b981",
    whiteSpace: "pre-wrap",
    overflowWrap: "anywhere",
  },
  inspectorEmpty: { margin: 0, color: "#9aac9e", fontSize: 12, lineHeight: 1.5 },
  modLoader: {
    marginTop: 14,
    paddingTop: 12,
    borderTop: "1px solid #4d6860",
  },
  modLoaderSummary: {
    cursor: "pointer",
    fontSize: 12,
    fontWeight: 650,
    lineHeight: 1.5,
  },
  modLoaderLabel: { display: "block", margin: "10px 0 5px", color: "#b4c5ba", fontSize: 11 },
  modProfileInput: {
    width: "100%",
    minHeight: 150,
    boxSizing: "border-box",
    resize: "vertical",
    padding: 8,
    border: "1px solid #4d6860",
    background: "#101c1d",
    color: "#e5efe8",
    font: "11px/1.45 ui-monospace, monospace",
  },
  modProfileButton: {
    marginTop: 8,
    padding: "7px 10px",
    border: "1px solid #70d6a4",
    background: "#183b35",
    color: "#f1f4ed",
    font: "inherit",
    fontSize: 11,
    cursor: "pointer",
  },
  modProfileList: { margin: "9px 0 0", paddingLeft: 16, color: "#b4c5ba", fontSize: 10, overflowWrap: "anywhere" },
  networkPortal: {
    marginTop: 14,
    paddingTop: 12,
    borderTop: "1px solid #4d6860",
  },
  networkPortalHelp: { margin: "8px 0", color: "#9aac9e", fontSize: 10, lineHeight: 1.45 },
  networkStatusTable: {
    width: "100%",
    margin: "8px 0",
    borderCollapse: "collapse",
    fontSize: 10,
  },
  networkStatusLabel: {
    padding: "4px 8px 4px 0",
    color: "#9aac9e",
    fontWeight: 500,
    textAlign: "left",
    verticalAlign: "top",
  },
  networkStatusValue: {
    padding: "4px 0",
    color: "#e5efe8",
    overflowWrap: "anywhere",
    textAlign: "left",
  },
  networkInput: {
    display: "block",
    width: "100%",
    boxSizing: "border-box",
    marginTop: 5,
    padding: 7,
    border: "1px solid #4d6860",
    background: "#101c1d",
    color: "#e5efe8",
    font: "10px ui-monospace, monospace",
  },
  networkButton: {
    margin: "4px 5px 4px 0",
    padding: "7px 10px",
    border: "1px solid #70d6a4",
    background: "#183b35",
    color: "#f1f4ed",
    font: "inherit",
    fontSize: 11,
    cursor: "pointer",
  },
  canvas: { width: "100%", height: "100%", display: "block" },
  healthOverlayLayer: {
    position: "absolute",
    inset: 1,
    zIndex: 1,
    overflow: "hidden",
    pointerEvents: "none",
  },
  healthTag: {
    position: "absolute",
    top: 0,
    left: 0,
    visibility: "hidden",
    transform: "translate3d(-10000px, -10000px, 0)",
    background: "rgba(0, 0, 0, 0.75)",
    padding: "2px 6px",
    borderRadius: 4,
    border: "1px solid #555",
    color: "#fff",
    fontSize: 10,
    fontFamily: "monospace",
    whiteSpace: "nowrap",
  },
  footer: { maxWidth: 1240, margin: "0 auto", display: "flex", alignItems: "center", gap: 22, paddingTop: 16, color: "#49635a", fontSize: 12 },
  hint: { marginLeft: "auto", textAlign: "right", color: "#688077" },
};
