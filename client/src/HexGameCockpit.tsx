import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { Canvas, type ThreeEvent } from "@react-three/fiber";
import { OrbitControls, Stats } from "@react-three/drei";
import * as THREE from "three";
import init, {
  wasm_get_current_state,
  wasm_initialize_match,
  wasm_process_token_for_match,
} from "./wasm/hex_game_core";

type Position = { q: number; r: number; z: number };
type EngineComponent = {
  type?: string;
  data?: {
    inventory_id?: string;
    face_texture?: string;
    side_texture?: string;
    labels?: string[];
  };
  Terrain?: {
    inventory_id?: string;
    face_texture?: string;
    side_texture?: string;
  };
  TerrainType?: { terrain?: string };
  Tags?: { labels?: string[] };
};
type EngineEntity = {
  id?: string;
  spatial?: Position;
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

const SOCKET_URL = "ws://127.0.0.1:8080";
const WASM_MATCH_ID = "wasm_sandbox";
const HEX_RADIUS = 1;
const PILLAR_HEIGHT = 0.42;

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

  return (
    <group position={[worldX, worldY, worldZ]} onClick={onSelect}>
      {selected && (
        <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.005, 0]}>
          <ringGeometry args={[0.3, 0.38, 32]} />
          <meshBasicMaterial color="#f0d47a" />
        </mesh>
      )}
      <mesh castShadow>
        <sphereGeometry args={[0.23, 20, 14]} />
        <meshStandardMaterial color={playerOne ? "#58a9e8" : "#f07868"} roughness={0.45} />
      </mesh>
    </group>
  );
}

type SceneProps = {
  terrain: TerrainData[];
  units: RenderEntity[];
  selectedUnitId: string | null;
  onSelectUnit: (entity: RenderEntity, event: ThreeEvent<MouseEvent>) => void;
  onHexClick: (q: number, r: number, z: number) => void;
};

function HexScene({ terrain, units, selectedUnitId, onSelectUnit, onHexClick }: SceneProps) {
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
              onHexClick(cell.spatial.q, cell.spatial.r, cell.spatial.z);
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
      <OrbitControls makeDefault target={[0, 0, 0]} enableDamping dampingFactor={0.08} />
      <Stats />
    </>
  );
}

export default function HexGameCockpit() {
  const [mode, setMode] = useState<Mode>("network");
  const [gameState, setGameState] = useState<EngineState | null>(null);
  const [selectedUnitId, setSelectedUnitId] = useState<string | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [notice, setNotice] = useState("Opening local engine stream");
  const [socket, setSocket] = useState<WebSocket | null>(null);

  useEffect(() => {
    let cancelled = false;
    let ws: WebSocket | null = null;

    if (mode === "wasm") {
      setSocket(null);
      setConnection("connecting");
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
    setNotice("Opening local engine stream");
    ws = new WebSocket(SOCKET_URL);
    setSocket(ws);
    ws.onopen = () => {
      if (!cancelled) {
        setConnection("connected");
        setNotice("Live engine stream connected");
      }
    };
    ws.onmessage = (event) => {
      if (cancelled) return;
      const message = String(event.data);
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
    ws.onerror = () => {
      if (!cancelled) setNotice("WebSocket error; check the local orchestrator");
    };
    ws.onclose = () => {
      if (!cancelled) {
        setConnection("offline");
        setSocket(null);
        setNotice("Engine stream disconnected");
      }
    };
    return () => {
      cancelled = true;
      ws?.close();
      setSocket((activeSocket) => activeSocket === ws ? null : activeSocket);
    };
  }, [mode]);

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

  const handleHexClick = (q: number, r: number, z: number) => {
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
    const nextId = selectedUnitId === unit.id ? null : unit.id;
    setSelectedUnitId(nextId);
    setNotice(nextId ? `${unit.id} selected` : "Selection cleared");
  };

  const chooseMode = (nextMode: Mode) => {
    if (nextMode === mode) return;
    setSelectedUnitId(null);
    setGameState(null);
    setMode(nextMode);
  };

  const playerOneCount = units.filter((unit) => {
    const labels = unitLabels(unit);
    return labels.includes("P1") || labels.includes("Player_1");
  }).length;
  const playerTwoCount = units.length - playerOneCount;

  return (
    <main style={styles.page}>
      <nav style={styles.modeBar} aria-label="Simulation mode">
        <button
          type="button"
          aria-pressed={mode === "network"}
          onClick={() => chooseMode("network")}
          style={{ ...styles.modeButton, ...(mode === "network" ? styles.modeButtonActive : {}) }}
        >
          Live Network Mode (WebSocket)
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
      </header>

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
            onSelectUnit={selectUnit}
            onHexClick={handleHexClick}
          />
        </Canvas>
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
    maxWidth: 1240,
    height: "min(70vh, 760px)",
    minHeight: 400,
    margin: "0 auto",
    border: "1px solid #344d47",
    background: "#101c1d",
    boxShadow: "0 16px 38px #18302d20",
    overflow: "hidden",
  },
  canvas: { width: "100%", height: "100%", display: "block" },
  footer: { maxWidth: 1240, margin: "0 auto", display: "flex", alignItems: "center", gap: 22, paddingTop: 16, color: "#49635a", fontSize: 12 },
  hint: { marginLeft: "auto", textAlign: "right", color: "#688077" },
};
