import { useEffect, useMemo, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { Canvas, useFrame, useThree, type ThreeEvent } from "@react-three/fiber";
import { OrbitControls, Stats } from "@react-three/drei";
import * as THREE from "three";
import { HEX_RADIUS, PILLAR_HEIGHT, PLAYER_COLORS } from "../constants";
import { unitHealth, unitLabels } from "../model";
import type { HealthLabelElements, RenderEntity, TerrainData } from "../types";

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
  attackTarget: boolean;
  onSelect: (event: ThreeEvent<MouseEvent>) => void;
};

function UnitMarker({ entity, selected, attackTarget, onSelect }: UnitMarkerProps) {
  const labels = unitLabels(entity);
  const playerOne = labels.includes("P1") || labels.includes("Player_1");
  const sizeX = HEX_RADIUS * Math.sqrt(3);
  const sizeY = HEX_RADIUS * 1.5;
  const worldX = sizeX * (entity.spatial.q + entity.spatial.r * 0.5);
  const worldZ = sizeY * entity.spatial.r;
  const worldY = entity.spatial.z * 0.5 + PILLAR_HEIGHT + 0.08;
  const visualPrimitive = entity.visual_primitive?.trim().toUpperCase();
  const isSphere = visualPrimitive === "SPHERE";
  const unitColor = playerOne ? PLAYER_COLORS.P1 : PLAYER_COLORS.P2;

  return (
    <group position={[worldX, worldY, worldZ]} onClick={onSelect}>
      {attackTarget && (
        <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.01, 0]}>
          <ringGeometry args={[0.48, 0.65, 6]} />
          <meshBasicMaterial color="#ff3333" toneMapped={false} side={THREE.DoubleSide} />
        </mesh>
      )}
      {selected && (
        <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.005, 0]}>
          <ringGeometry args={[0.3, 0.38, 32]} />
          <meshBasicMaterial color="#f0d47a" />
        </mesh>
      )}
      {isSphere ? (
        <mesh castShadow position={[0, 0.3, 0]}>
          <sphereGeometry args={[0.3, 16, 16]} />
          <meshStandardMaterial color={unitColor} emissive={attackTarget ? "#ff3333" : "#000000"} emissiveIntensity={0.65} />
        </mesh>
      ) : (
        <mesh castShadow position={[0, 0.3, 0]}>
          <boxGeometry args={[0.4, 0.4, 0.4]} />
          <meshStandardMaterial color={unitColor} emissive={attackTarget ? "#ff3333" : "#000000"} emissiveIntensity={0.65} />
        </mesh>
      )}
    </group>
  );
}

function UnitHealthProjection({
  units,
  target,
  labelElements,
}: {
  units: RenderEntity[];
  target: THREE.Vector3;
  labelElements: HealthLabelElements;
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

  return null;
}

type SceneProps = {
  terrain: TerrainData[];
  units: RenderEntity[];
  selectedUnitId: string | null;
  attackTargetIds: Set<string>;
  healthLabelElements: HealthLabelElements;
  onSelectUnit: (entity: RenderEntity, event: ThreeEvent<MouseEvent>) => void;
  onHexClick: (entity: TerrainData) => void;
};

function HexScene({
  terrain,
  units,
  selectedUnitId,
  attackTargetIds,
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
            attackTarget={attackTargetIds.has(unit.id)}
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

export type HexBoardProps = SceneProps & {
  onContextMenu: (event: ReactMouseEvent<HTMLElement>) => void;
  children?: ReactNode;
};

export function HexBoard({
  terrain,
  units,
  selectedUnitId,
  attackTargetIds,
  healthLabelElements,
  onSelectUnit,
  onHexClick,
  onContextMenu,
  children,
}: HexBoardProps) {
  return (
    <section
      style={styles.boardFrame}
      aria-label="3D tactical board"
      onContextMenu={onContextMenu}
    >
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
          attackTargetIds={attackTargetIds}
          healthLabelElements={healthLabelElements}
          onSelectUnit={onSelectUnit}
          onHexClick={onHexClick}
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
              data-attack-target={attackTargetIds.has(unit.id) ? "true" : undefined}
              data-selected={selectedUnitId === unit.id ? "true" : undefined}
              style={{
                ...styles.healthTag,
                ...(attackTargetIds.has(unit.id) ? styles.targetHealthTag : {}),
              }}
            >
              {attackTargetIds.has(unit.id) ? "TARGET · " : ""}
              {health ? `${health.currentLife} / ${health.maxLife} HP` : "HP unavailable"}
            </div>
          );
        })}
      </div>
      {children}
    </section>
  );
}

const styles: Record<string, CSSProperties> = {
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
  targetHealthTag: {
    border: "2px solid #ff3333",
    boxShadow: "0 0 12px #ff3333",
    color: "#fff",
  },
};
