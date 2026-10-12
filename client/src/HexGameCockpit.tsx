import { useEffect, useMemo, useState, type CSSProperties } from "react";

type Position = { q: number; r: number; z: number };
type EngineComponent = {
  type?: string;
  data?: { inventory_id?: string; labels?: string[] };
  Terrain?: { inventory_id?: string };
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

const SOCKET_URL = "ws://127.0.0.1:8080";
const HEX_SIZE = 30;
const EDGE = 46;

function componentList(entity: EngineEntity): EngineComponent[] {
  return Array.isArray(entity.components)
    ? entity.components
    : Object.values(entity.components ?? {});
}

function hasComponent(entity: EngineEntity, name: string): boolean {
  return componentList(entity).some(
    (component) => component.type === name || name in component,
  );
}

function unitLabels(entity: EngineEntity): string[] {
  const tagged = componentList(entity).find(
    (component) => component.type === "Tags" || "Tags" in component,
  );
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

function axialToPixel(q: number, r: number, size: number) {
  return {
    x: size * (Math.sqrt(3) * q + (Math.sqrt(3) / 2) * r),
    y: size * (1.5 * r),
  };
}

function hexPoints(cx: number, cy: number, size: number): string {
  return Array.from({ length: 6 }, (_, index) => {
    const angle = (Math.PI / 3) * index;
    return `${cx + size * Math.cos(angle)},${cy + size * Math.sin(angle)}`;
  }).join(" ");
}

export default function HexGameCockpit() {
  const [gameState, setGameState] = useState<EngineState | null>(null);
  const [selectedUnitId, setSelectedUnitId] = useState<string | null>(null);
  const [connection, setConnection] = useState<"connecting" | "connected" | "offline">("connecting");
  const [notice, setNotice] = useState("Opening local engine stream");
  const [socket, setSocket] = useState<WebSocket | null>(null);

  useEffect(() => {
    const ws = new WebSocket(SOCKET_URL);
    setSocket(ws);
    ws.onopen = () => {
      setConnection("connected");
      setNotice("Live engine stream connected");
    };
    ws.onmessage = (event) => {
      const message = String(event.data);
      if (message.startsWith("E:")) {
        setNotice(message.slice(2));
        return;
      }
      try {
        setGameState(JSON.parse(message) as EngineState);
        setNotice("State synchronized");
      } catch {
        setNotice(`Unrecognized engine response: ${message.slice(0, 100)}`);
      }
    };
    ws.onerror = () => setNotice("WebSocket error; check the local orchestrator");
    ws.onclose = () => {
      setConnection("offline");
      setSocket(null);
      setNotice("Engine stream disconnected");
    };
    return () => {
      ws.close();
    };
  }, []);

  const entities = useMemo(() => entitiesFromState(gameState), [gameState]);
  const terrain = useMemo(
    () => entities.filter((entity) => hasComponent(entity, "Terrain") || hasComponent(entity, "TerrainType")),
    [entities],
  );
  const units = useMemo(
    () => entities.filter((entity) => unitLabels(entity).includes("Unit")),
    [entities],
  );

  const terrainCells = useMemo(() => {
    const unique = new Map<string, RenderEntity>();
    for (const cell of terrain) {
      unique.set(`${cell.spatial.q}:${cell.spatial.r}:${cell.spatial.z}`, cell);
    }
    return [...unique.values()];
  }, [terrain]);

  const layout = useMemo(() => {
    const centers = terrainCells.map((cell) => ({
      cell,
      point: axialToPixel(cell.spatial.q, cell.spatial.r, HEX_SIZE),
    }));
    if (centers.length === 0) {
      return { centers, width: 640, height: 420, pointFor: (_position: Position) => ({ x: 320, y: 210 }) };
    }
    const minX = Math.min(...centers.map(({ point }) => point.x - HEX_SIZE));
    const maxX = Math.max(...centers.map(({ point }) => point.x + HEX_SIZE));
    const minY = Math.min(...centers.map(({ point }) => point.y - HEX_SIZE));
    const maxY = Math.max(...centers.map(({ point }) => point.y + HEX_SIZE));
    const pointFor = (position: Position) => {
      const point = axialToPixel(position.q, position.r, HEX_SIZE);
      return { x: point.x - minX + EDGE, y: point.y - minY + EDGE };
    };
    return {
      centers,
      width: maxX - minX + EDGE * 2,
      height: maxY - minY + EDGE * 2,
      pointFor,
    };
  }, [terrainCells]);

  const moveSelectedUnit = (cell: RenderEntity) => {
    if (!selectedUnitId) return;
    const occupied = units.some(
      (unit) =>
        unit.id !== selectedUnitId &&
        unit.spatial.q === cell.spatial.q &&
        unit.spatial.r === cell.spatial.r &&
        unit.spatial.z === cell.spatial.z,
    );
    if (occupied) {
      setNotice("Move rejected locally: that hex is occupied");
      return;
    }
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      setNotice("Move not sent: engine stream is offline");
      return;
    }
    const token = `M:${selectedUnitId}:Q${cell.spatial.q}R${cell.spatial.r}Z${cell.spatial.z}`;
    socket.send(token);
    setNotice(`Sent ${token}`);
    setSelectedUnitId(null);
  };

  return (
    <main style={styles.page}>
      <header style={styles.header}>
        <div>
          <p style={styles.kicker}>HEXGAMESIM / LIVE SESSION</p>
          <h1 style={styles.title}>Tactical board</h1>
        </div>
        <div style={styles.connection}>
          <span style={{ ...styles.statusDot, background: connection === "connected" ? "#70d6a4" : connection === "connecting" ? "#edbd62" : "#ef7777" }} />
          <span>{connection}</span>
          <span style={styles.matchId}>{gameState?.context?.match_id ?? "waiting for state"}</span>
        </div>
      </header>

      <section style={styles.toolbar} aria-live="polite">
        <span style={styles.notice}>{notice}</span>
        <span style={styles.counts}>{terrainCells.length} hexes <i /> {units.length} units</span>
      </section>

      <section style={styles.boardFrame}>
        <svg
          viewBox={`0 0 ${layout.width} ${layout.height}`}
          role="img"
          aria-label="Live tactical hex board"
          style={styles.board}
        >
          <defs>
            <pattern id="cockpit-grid" width="24" height="24" patternUnits="userSpaceOnUse">
              <path d="M 24 0 L 0 0 0 24" fill="none" stroke="#263b3b" strokeWidth="0.7" opacity="0.45" />
            </pattern>
          </defs>
          <rect width={layout.width} height={layout.height} fill="#101c1d" />
          <rect width={layout.width} height={layout.height} fill="url(#cockpit-grid)" />
          {layout.centers.map(({ cell }) => {
            const { x, y } = layout.pointFor(cell.spatial);
            const selected = selectedUnitId !== null;
            const blocked = units.some(
              (unit) => unit.id !== selectedUnitId && unit.spatial.q === cell.spatial.q && unit.spatial.r === cell.spatial.r && unit.spatial.z === cell.spatial.z,
            );
            return (
              <g key={cell.id} onClick={() => moveSelectedUnit(cell)} style={{ cursor: selected && !blocked ? "pointer" : "default" }}>
                <polygon
                  points={hexPoints(x, y, HEX_SIZE)}
                  fill={selected && !blocked ? "#23443e" : "#1d3432"}
                  stroke={selected && !blocked ? "#72b99b" : "#526e66"}
                  strokeWidth="1.5"
                />
                <text x={x} y={y + 4} textAnchor="middle" fill="#9ab1a7" fontSize="9" fontFamily="ui-monospace, monospace">
                  {cell.spatial.q},{cell.spatial.r}
                </text>
              </g>
            );
          })}
          {units.map((unit) => {
            const { x, y } = layout.pointFor(unit.spatial);
            const labels = unitLabels(unit);
            const playerOne = labels.includes("P1") || labels.includes("Player_1");
            const color = playerOne ? "#58a9e8" : "#f07868";
            const isSelected = selectedUnitId === unit.id;
            return (
              <g
                key={unit.id}
                transform={`translate(${x} ${y})`}
                role="button"
                tabIndex={0}
                aria-label={`Select ${unit.id}`}
                onClick={(event) => {
                  event.stopPropagation();
                  setSelectedUnitId(isSelected ? null : unit.id);
                  setNotice(isSelected ? "Selection cleared" : `${unit.id} selected`);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    event.stopPropagation();
                    setSelectedUnitId(isSelected ? null : unit.id);
                  }
                }}
                style={{ cursor: "pointer", outline: "none" }}
              >
                {isSelected && <circle r="17" fill="none" stroke="#f0d47a" strokeWidth="2" />}
                <circle r="10" fill={color} stroke="#e8eee6" strokeWidth="1.5" />
                <text x="0" y="25" textAnchor="middle" fill="#e6ece5" fontSize="10" fontFamily="ui-monospace, monospace">
                  {unit.id.replace("UNIT_", "")}
                </text>
              </g>
            );
          })}
        </svg>
      </section>

      <footer style={styles.footer}>
        <span><b style={{ color: "#58a9e8" }}>P1</b> Player 1</span>
        <span><b style={{ color: "#f07868" }}>P2</b> Player 2</span>
        <span style={styles.hint}>{selectedUnitId ? `Selected: ${selectedUnitId} · click an open hex to move` : "Select a unit to issue a move"}</span>
      </footer>
    </main>
  );
}

const styles: Record<string, CSSProperties> = {
  page: {
    minHeight: "100vh",
    padding: "28px clamp(16px, 4vw, 56px)",
    background: "#e9eee8",
    color: "#18302d",
    fontFamily: "'Avenir Next', Avenir, 'Segoe UI', sans-serif",
    boxSizing: "border-box",
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
  boardFrame: { maxWidth: 1240, margin: "0 auto", border: "1px solid #344d47", background: "#101c1d", boxShadow: "0 16px 38px #18302d20", overflow: "auto" },
  board: { display: "block", width: "100%", minWidth: 360, maxHeight: "72vh", margin: "0 auto" },
  footer: { maxWidth: 1240, margin: "0 auto", display: "flex", alignItems: "center", gap: 22, paddingTop: 16, color: "#49635a", fontSize: 12 },
  hint: { marginLeft: "auto", textAlign: "right", color: "#688077" },
};
