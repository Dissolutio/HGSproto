import { useEffect, useMemo, useRef, useState } from "react";
import type { ThreeEvent } from "@react-three/fiber";
import init, {
  wasm_deploy_profile_unit,
  wasm_export_local_sandbox,
  wasm_get_current_state,
  wasm_initialize_match,
  wasm_load_mod_profiles,
  wasm_process_token_for_match,
  wasm_reset_match,
  wasm_restore_local_sandbox,
} from "../../wasm/hex_game_core";
import {
  ACTIVE_MATCH_ID_STORAGE_KEY,
  CACHED_MANAGER_ID_STORAGE_KEY,
  getTabId,
  MOCK_MAP_JSON,
  readRecoverySession,
  SOCKET_URL,
  WASM_MATCH_ID,
} from "../constants";
import {
  entitiesFromState,
  findComponent,
  isAdjacent,
  isRecord,
  parseState,
  summarizeProfiles,
  terrainFromEntities,
  unitLabels,
  unitSeat,
} from "../model";
import {
  discardSavedSandbox,
  openSandboxStorage,
  restoreSavedSandbox,
  saveSandbox,
  type SandboxStorage,
} from "../sandboxPersistence";
import type {
  ConnectionState,
  EngineState,
  ModProfileSummary,
  Mode,
  PlayerSeat,
  RecoverySession,
  RenderEntity,
  TerrainData,
} from "../types";

export function useGameRuntime() {
  const [tabId] = useState(getTabId);
  const [mode, setMode] = useState<Mode>("wasm");
  const [gameState, setGameState] = useState<EngineState | null>(null);
  const [selectedUnitId, setSelectedUnitId] = useState<string | null>(null);
  const [actionPending, setActionPending] = useState(false);
  const [endTurnAfterSkip, setEndTurnAfterSkip] = useState<{
    seat: string;
    matchId: string;
  } | null>(null);
  const [inspectedEntity, setInspectedEntity] = useState<RenderEntity | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [notice, setNotice] = useState("Loading local WebAssembly engine");
  const [socket, setSocket] = useState<WebSocket | null>(null);
  const [modProfileText, setModProfileText] = useState("");
  const [loadedModProfiles, setLoadedModProfiles] = useState<ModProfileSummary[]>([]);
  const [selectedModId, setSelectedModId] = useState("");
  const [deploymentSeat, setDeploymentSeat] = useState<PlayerSeat>("P1");
  const [deploymentSlotId, setDeploymentSlotId] = useState("UNIT_P1_1");
  const [localSeat, setLocalSeat] = useState<PlayerSeat | null>(null);
  const [startingHealth, setStartingHealth] = useState("");
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
  const sandboxStorage = useRef<SandboxStorage | null>(null);
  const sandboxRestoreAttempted = useRef(false);
  const sandboxSavingEnabled = useRef(false);

  const showLoadedProfiles = (summaries: ModProfileSummary[], append: boolean) => {
    setLoadedModProfiles((current) => append ? [...current, ...summaries] : summaries);
    setSelectedModId(summaries[0]?.modId ?? "");
    setStartingHealth(summaries[0]?.currentHealth.toString() ?? "");
  };

  const restoreLocalSandboxOnce = () => {
    if (sandboxRestoreAttempted.current) return null;
    sandboxRestoreAttempted.current = true;
    const storage = openSandboxStorage(() => window.sessionStorage);
    sandboxStorage.current = storage;
    const startup = restoreSavedSandbox(
      storage,
      (record) => wasm_restore_local_sandbox(WASM_MATCH_ID, record),
    );
    sandboxSavingEnabled.current = startup.savingEnabled;
    if (startup.kind === "empty") return null;
    if (startup.kind !== "restored") return startup.message;
    try {
      const record: unknown = JSON.parse(startup.record);
      const profiles = isRecord(record) && Array.isArray(record.profiles) ? record.profiles : [];
      showLoadedProfiles(summarizeProfiles(profiles), false);
    } catch (error) {
      return `Saved sandbox restored, but its profile list could not be shown: ${error instanceof Error ? error.message : String(error)}`;
    }
    return `Saved sandbox restored with ${startup.profileCount} profile${startup.profileCount === 1 ? "" : "s"}`;
  };

  const saveLocalSandbox = () => {
    if (!sandboxSavingEnabled.current) {
      return sandboxRestoreAttempted.current
        ? " · not saved for refresh (saving is off for this page)"
        : "";
    }
    const warning = saveSandbox(
      sandboxStorage.current,
      () => wasm_export_local_sandbox(WASM_MATCH_ID),
    );
    return warning ? ` · ${warning}` : "";
  };

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
          const restoreNotice = restoreLocalSandboxOnce();
          // Leaves a restored or already running sandbox match unchanged.
          wasm_initialize_match(WASM_MATCH_ID, MOCK_MAP_JSON);
          const state = parseState(wasm_get_current_state(WASM_MATCH_ID));
          if (cancelled) return;
          setGameState(state);
          setConnection("ready");
          setNotice(restoreNotice ?? "Autonomous WASM sandbox ready");
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
          setLocalSeat("P1");
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
          setLocalSeat("P2");
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
          setLocalSeat(role === "HOST" ? "P1" : role === "JOIN" ? "P2" : null);
          setConnection("connected");
          setRecoverySession(null);
          reconnectAttempts.current = 0;
          setNotice(
            `Reconnected; restored match state and ${recoveredActionCount} recorded action${recoveredActionCount === 1 ? "" : "s"}`,
          );
          return;
        }
        if (message === "P2P_RESET") {
          setActionPending(false);
          setEndTurnAfterSkip(null);
          clearActiveSession();
          setLocalNodeId(null);
          setLocalMatchId(null);
          setLocalSeat(null);
          setRemoteNodeId(null);
          setIsPeerConnected(false);
          setJoinNodeId("");
          setSelectedUnitId(null);
          setInspectedEntity(null);
          triggerFreshMapSnapshotLoad(false);
          return;
        }
        if (message === "E:SESSION_EXPIRED") {
          setActionPending(false);
          setEndTurnAfterSkip(null);
          clearActiveSession();
          setLocalNodeId(null);
          setLocalMatchId(null);
          setLocalSeat(null);
          setRemoteNodeId(null);
          setIsPeerConnected(false);
          setNotice("The previous match session expired; reconnecting with a fresh session");
          gatewaySocket.close();
          return;
        }
        if (message.startsWith("E:")) {
          setActionPending(false);
          setEndTurnAfterSkip(null);
          setNotice(message.slice(2));
          return;
        }
        try {
          setGameState(parseState(message));
          setActionPending(false);
          setNotice("State synchronized");
        } catch (error) {
          setActionPending(false);
          setEndTurnAfterSkip(null);
          setNotice(`Invalid engine response: ${error instanceof Error ? error.message : String(error)}`);
        }
      };
      gatewaySocket.onerror = () => {
        if (!cancelled) {
          setIsPeerConnected(false);
          setNotice(
            `Cannot reach the local WebSocket gateway at ${SOCKET_URL}. Start it with "cargo run -p hex_game_orchestrator" from the repository root. Retrying automatically.`,
          );
        }
      };
      gatewaySocket.onclose = () => {
        if (cancelled) return;
        setActionPending(false);
        setEndTurnAfterSkip(null);
        setConnection("offline");
        setIsPeerConnected(false);
        const storedSession = readRecoverySession();
        if (storedSession) setRecoverySession(storedSession);
        setNotice(
          storedSession
            ? `Local gateway disconnected; preserving match details and retrying. If it is not running, start it with "cargo run -p hex_game_orchestrator" from the repository root.`
            : `Local WebSocket gateway is unavailable at ${SOCKET_URL}. Start it with "cargo run -p hex_game_orchestrator" from the repository root. Retrying automatically.`,
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
  useEffect(() => {
    const inspectedId = inspectedEntity?.id;
    if (!inspectedId) return;
    setInspectedEntity(
      entities.find((entity) => entity.id === inspectedId) ?? null,
    );
  }, [entities, inspectedEntity?.id]);

  const terrain = useMemo(() => terrainFromEntities(entities), [entities]);
  const units = useMemo(
    () => entities.filter((entity) => unitLabels(entity).includes("Unit")),
    [entities],
  );
  const deploymentSlots = useMemo(
    () =>
      units
        .filter((unit) => unitLabels(unit).includes(deploymentSeat))
        .sort((left, right) => left.id.localeCompare(right.id)),
    [units, deploymentSeat],
  );
  const deploymentSlot =
    deploymentSlots.find((unit) => unit.id === deploymentSlotId) ?? deploymentSlots[0] ?? null;
  const selectedProfile = loadedModProfiles.find((profile) => profile.modId === selectedModId);
  const activeSeat = gameState?.context?.active_player;
  const turnPhase = gameState?.context?.turn_phase;
  const winner = gameState?.context?.winner;
  const playerSeat = mode === "wasm"
    ? (activeSeat === "P1" || activeSeat === "P2" ? activeSeat : null)
    : localSeat;
  const isYourTurn = !!playerSeat && playerSeat === activeSeat && !winner;
  const canAct = isYourTurn && !actionPending &&
    (mode === "wasm" ? connection === "ready" : connection === "connected");
  const selectedUnit = units.find((unit) => unit.id === selectedUnitId);
  const attackTargetIds = useMemo(() => {
    if (!canAct || turnPhase !== "attack" || !selectedUnit || unitSeat(selectedUnit) !== activeSeat) {
      return new Set<string>();
    }
    return new Set(units.filter((unit) => {
      const seat = unitSeat(unit);
      return seat !== null && seat !== activeSeat &&
        !!findComponent(unit, "Destructible") &&
        isAdjacent(selectedUnit.spatial, unit.spatial);
    }).map((unit) => unit.id));
  }, [canAct, turnPhase, selectedUnit, activeSeat, units]);

  useEffect(() => {
    if (!canAct || !selectedUnit || unitSeat(selectedUnit) !== activeSeat ||
      !["movement", "attack"].includes(turnPhase ?? "")) {
      setSelectedUnitId(null);
    }
  }, [canAct, selectedUnit, activeSeat, turnPhase]);

  const clearSelection = () => {
    setSelectedUnitId(null);
    setNotice("Selection cleared");
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSelectedUnitId(null);
        setNotice("Selection cleared");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

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

  const submitAction = (token: string) => {
    if (!canAct) {
      setNotice(actionPending
        ? "Waiting for the referee to confirm the previous action"
        : winner ? "Match is finished" : !isYourTurn
          ? "Action unavailable: wait for your assigned seat's turn"
          : "Action not sent: engine stream is offline");
      return false;
    }
    if (mode === "wasm") {
      const actingSeat = gameState?.context?.active_player;
      if (!actingSeat) {
        setNotice("Referee action rejected: active seat is missing from the match state");
        return false;
      }
      try {
        const result = wasm_process_token_for_match(WASM_MATCH_ID, actingSeat, token);
        if (result.startsWith("E:")) {
          console.error("Referee Rejected Action:", result);
          setNotice(result.slice(2));
          return false;
        }
        setSelectedUnitId(null);
        sync3DCanvasWithWasmState();
        setNotice(`Applied locally: ${token}${saveLocalSandbox()}`);
        return true;
      } catch (error) {
        setNotice(`WASM action failed: ${error instanceof Error ? error.message : String(error)}`);
        return false;
      }
    }
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      setNotice("Action not sent: engine stream is offline");
      return false;
    }
    try {
      socket.send(token);
      setActionPending(true);
      setNotice(`Sent ${token}`);
      setSelectedUnitId(null);
      return true;
    } catch (error) {
      setNotice(`Action could not be sent: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    }
  };

  useEffect(() => {
    if (!endTurnAfterSkip || actionPending) return;
    if (winner || activeSeat !== endTurnAfterSkip.seat ||
      gameState?.context?.match_id !== endTurnAfterSkip.matchId) {
      setEndTurnAfterSkip(null);
      return;
    }
    if (turnPhase === "end") {
      setEndTurnAfterSkip(null);
      submitAction("E");
    }
  }, [gameState, endTurnAfterSkip, actionPending]);

  const handleEndTurn = () => {
    if (turnPhase !== "attack") {
      submitAction("E");
      return;
    }
    if (!activeSeat || !gameState?.context?.match_id) {
      setNotice("Cannot skip attack: match identity is unavailable");
      return;
    }
    setEndTurnAfterSkip({ seat: activeSeat, matchId: gameState.context.match_id });
    if (!submitAction("S")) setEndTurnAfterSkip(null);
  };

  const handleUnitClick = (unit: RenderEntity) => {
    setInspectedEntity(unit);
    if (!canAct || !["movement", "attack"].includes(turnPhase ?? "")) {
      setNotice(winner ? "Match is finished" : "Unit inspected; turn actions are unavailable");
      return;
    }
    if (attackTargetIds.has(unit.id) && selectedUnitId) {
      submitAction(`A:${selectedUnitId}:${unit.id}`);
      return;
    }
    if (unitSeat(unit) !== activeSeat) {
      setNotice(turnPhase === "attack"
        ? "Select a friendly unit, then click a red adjacent enemy target"
        : "Select a unit belonging to your active seat");
      return;
    }
    const nextId = selectedUnitId === unit.id ? null : unit.id;
    setSelectedUnitId(nextId);
    setNotice(nextId ? `${unit.id} selected` : "Selection cleared");
  };

  const handleHexClick = (cell: TerrainData) => {
    setInspectedEntity(entities.find((entity) => entity.id === cell.id) ?? cell);
    const { q, r, z } = cell.spatial;
    const occupant = findEntityAtCoordinate(q, r, z);
    if (occupant) {
      handleUnitClick(occupant);
      return;
    }
    if (canAct && turnPhase === "movement" && selectedUnitId) {
      submitAction(`M:${selectedUnitId}:Q${q}R${r}Z${z}`);
    }
  };

  const selectUnit = (unit: RenderEntity, event: ThreeEvent<MouseEvent>) => {
    event.stopPropagation();
    handleUnitClick(unit);
  };

  const chooseMode = (nextMode: Mode) => {
    if (nextMode === mode) return;
    setSelectedUnitId(null);
    setActionPending(false);
    setEndTurnAfterSkip(null);
    setInspectedEntity(null);
    setGameState(null);
    setLocalSeat(null);
    setMode(nextMode);
  };

  const loadModProfiles = () => {
    if (!modProfileText.trim()) {
      setNotice("Paste a mod profile before loading");
      return;
    }
    try {
      const parsed: unknown = JSON.parse(modProfileText);
      const summaries = summarizeProfiles(Array.isArray(parsed) ? parsed : [parsed]);
      const loadedCount = wasm_load_mod_profiles(modProfileText);
      showLoadedProfiles(summaries, true);
      setNotice(
        `Loaded ${loadedCount} community mod profile${loadedCount === 1 ? "" : "s"}${saveLocalSandbox()}`,
      );
    } catch (error) {
      setNotice(
        `Mod profile load failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  };

  const handleDeployProfile = () => {
    if (mode !== "wasm" || connection !== "ready") {
      setNotice("Switch to Autonomous Sandbox Mode before deploying a profile");
      return;
    }
    if (!selectedProfile || !deploymentSlot) {
      setNotice("Select a loaded unit profile and a roster slot first");
      return;
    }
    const life = Number(startingHealth);
    if (
      !Number.isInteger(life) ||
      life < 1 ||
      life > selectedProfile.maxHealth
    ) {
      setNotice(`Starting health must be a whole number from 1 to ${selectedProfile.maxHealth}`);
      return;
    }

    try {
      const displayName = wasm_deploy_profile_unit(
        WASM_MATCH_ID,
        deploymentSlot.id,
        selectedProfile.modId,
        deploymentSeat,
        life,
      );
      const state = parseState(wasm_get_current_state(WASM_MATCH_ID));
      setGameState(state);
      setInspectedEntity(
        entitiesFromState(state).find((entity) => entity.id === deploymentSlot.id) ?? null,
      );
      setSelectedUnitId(null);
      setNotice(
        `Deployed ${displayName} in ${deploymentSlot.id} for ${deploymentSeat} at ${life}/${selectedProfile.maxHealth} HP${saveLocalSandbox()}`,
      );
    } catch (error) {
      setNotice(
        `Unit deployment failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
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
        const saveNotice = saveLocalSandbox();
        if (mode === "network") {
          setMode("wasm");
          setNotice(`Native gateway unavailable; local WASM sandbox refreshed${saveNotice}`);
        } else {
          setNotice(`Fresh sandbox map loaded${saveNotice}`);
        }
        setConnection("ready");
      } catch (error) {
        setConnection("offline");
        setNotice(`Sandbox reset failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    })();
  };

  const handleDisconnectAndReset = () => {
    setActionPending(false);
    setEndTurnAfterSkip(null);
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
    setLocalSeat(null);
    setRemoteNodeId(null);
    setIsPeerConnected(false);
    setJoinNodeId("");
    setSelectedUnitId(null);
    setInspectedEntity(null);
    triggerFreshMapSnapshotLoad(gatewayResetRequested);
  };

  const handleStartFreshSandbox = () => {
    if (!window.confirm(
      "Start fresh? This discards the saved local sandbox, its imported profiles, and the current local match.",
    )) return;
    const error = discardSavedSandbox(
      sandboxStorage.current ?? openSandboxStorage(() => window.sessionStorage),
    );
    if (error) {
      setNotice(error);
      return;
    }
    window.location.reload();
  };

  const playerOneCount = units.filter((unit) => {
    const labels = unitLabels(unit);
    return labels.includes("P1") || labels.includes("Player_1");
  }).length;
  const playerTwoCount = units.length - playerOneCount;
  const ruleHint = gameState?.context?.winner
    ? `${gameState.context.winner} won by elimination`
    : gameState?.context?.turn_phase === "movement"
      ? "Move one friendly unit one adjacent hex, or skip movement"
      : gameState?.context?.turn_phase === "attack"
        ? selectedUnit
          ? "Attack one adjacent enemy for 1 damage, or skip attack"
          : "Select a friendly unit to attack for 1 damage, or skip attack"
        : gameState?.context?.turn_phase === "end"
          ? "End the turn to activate the other seat"
          : "Referee state unavailable";
  const storedRecoverySession = recoverySession ? readRecoverySession() : null;
  const showRecoveryBanner =
    mode === "network" &&
    !recoveryDismissed &&
    !!storedRecoverySession &&
    storedRecoverySession.matchId === recoverySession?.matchId &&
    socket?.readyState === WebSocket.CLOSED;


  return {
    state: {
      mode, gameState, selectedUnitId, actionPending, endTurnAfterSkip, inspectedEntity,
      connection, notice, socket, localSeat, localNodeId, localMatchId, remoteNodeId,
      isPeerConnected, joinNodeId, setJoinNodeId, recoverySession, recoveryDismissed, setRecoveryDismissed,
    },
    view: {
      entities, terrain, units, deploymentSlots, deploymentSlot, selectedProfile, activeSeat,
      turnPhase, winner, playerSeat, isYourTurn, canAct, selectedUnit, attackTargetIds,
      playerOneCount, playerTwoCount, ruleHint, showRecoveryBanner, healthLabelElements,
    },
    profiles: {
      modProfileText, setModProfileText, loadedModProfiles, setSelectedModId,
      deploymentSeat, setDeploymentSeat, setDeploymentSlotId, startingHealth,
      setStartingHealth, loadModProfiles, handleDeployProfile, handleStartFreshSandbox,
    },
    actions: {
      chooseMode, clearSelection, submitAction, handleEndTurn, selectUnit, handleHexClick,
      handleGracefulReconnection, handleHostMatch, handleJoinMatch, handleDisconnectAndReset,
    },
  };
}
