import type { ConnectionState, Mode, PlayerSeat, RecoverySession } from "../types";
import { PLAYER_COLORS, WASM_MATCH_ID } from "../constants";

type GameHudProps = {
  mode: Mode;
  onModeChange: (mode: Mode) => void;
  connection: ConnectionState;
  matchId?: string;
  isPeerConnected: boolean;
  recoverySession: RecoverySession | null;
  showRecoveryBanner: boolean;
  onReconnect: () => void;
  onDismissRecovery: () => void;
  notice: string;
  terrainCount: number;
  unitCount: number;
  playerSeat: PlayerSeat | null;
  activeSeat?: string;
  turnPhase?: string;
  winner?: string;
  hasGameState: boolean;
  isYourTurn: boolean;
  actionPending: boolean;
  canAct: boolean;
  selectedUnitId: string | null;
  attackTargetCount: number;
  ruleHint: string;
  endTurnPending: boolean;
  onClearSelection: () => void;
  onSkipMovement: () => void;
  onEndTurn: () => void;
};

export function GameHud({
  mode,
  onModeChange,
  connection,
  matchId,
  isPeerConnected,
  recoverySession,
  showRecoveryBanner,
  onReconnect,
  onDismissRecovery,
  notice,
  terrainCount,
  unitCount,
  playerSeat,
  activeSeat,
  turnPhase,
  winner,
  hasGameState,
  isYourTurn,
  actionPending,
  canAct,
  selectedUnitId,
  attackTargetCount,
  ruleHint,
  endTurnPending,
  onClearSelection,
  onSkipMovement,
  onEndTurn,
}: GameHudProps) {
  return (
    <>
      <nav style={styles.modeBar} aria-label="Simulation mode">
        <button
          type="button"
          aria-pressed={mode === "network"}
          onClick={() => onModeChange("network")}
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
          onClick={() => onModeChange("wasm")}
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
        <div style={styles.headerStatus}>
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
              {matchId ?? (mode === "wasm" ? WASM_MATCH_ID : "waiting for state")}
            </span>
          </div>
          <div
            style={{ ...styles.connection, textTransform: "none" }}
            role="status"
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
        <aside role="alert" style={styles.recoveryBanner}>
          <span>⚠️ Unexpected Disconnect Detected from Match {recoverySession.matchId}</span>
          <button type="button" onClick={onReconnect} style={styles.reconnectButton}>
            🔌 Attempt Graceful Reconnection
          </button>
          <button
            type="button"
            aria-label="Dismiss disconnect notification"
            onClick={onDismissRecovery}
            style={styles.dismissButton}
          >
            ×
          </button>
        </aside>
      )}

      <section style={styles.toolbar} aria-live="polite">
        <span style={styles.notice}>{notice}</span>
        <span style={styles.counts}>{terrainCount} hexes <i /> {unitCount} units</span>
      </section>

      <section style={styles.rulesBar} aria-label="Referee turn controls">
        <span
          style={{ ...styles.playerIdentity, borderColor: playerSeat ? PLAYER_COLORS[playerSeat] : "#94a3b8" }}
          aria-label="Player identity"
        >
          <span aria-hidden="true" style={{
            ...styles.playerColorDot,
            background: playerSeat ? PLAYER_COLORS[playerSeat] : "#94a3b8",
          }} />
          {playerSeat
            ? `${mode === "wasm" ? "Local hotseat" : "Your seat"}: ${playerSeat} · ${playerSeat === "P1" ? "Blue" : "Red"}`
            : "Your seat: unassigned · Color: pending"}
        </span>
        <div
          role="status"
          aria-label="Turn status"
          style={{
            ...styles.turnBanner,
            background: winner ? "#18302d" : isYourTurn ? "#183b35" : "#475569",
          }}
        >
          <strong>
            {winner ? `${winner} wins — match finished`
              : !hasGameState ? "Loading turn state"
                : !playerSeat ? "Awaiting seat assignment"
                  : isYourTurn ? "Your Turn" : "Waiting for Opponent"}
          </strong>
          <span style={styles.turnDetails}>
            Active seat: {activeSeat ?? "unavailable"} · Phase: {turnPhase ?? "unavailable"}
            {actionPending ? " · Confirming action..." : !canAct && isYourTurn ? " · Connection unavailable" : ""}
          </span>
        </div>
        <div style={styles.rulesButtons}>
          <button
            type="button"
            onClick={onClearSelection}
            disabled={!selectedUnitId}
            style={styles.ruleButton}
          >
            Cancel selection
          </button>
          <button
            type="button"
            onClick={onSkipMovement}
            disabled={!canAct || turnPhase !== "movement"}
            style={styles.ruleButton}
          >
            Skip movement
          </button>
          <button
            type="button"
            onClick={onEndTurn}
            disabled={!canAct || !["attack", "end"].includes(turnPhase ?? "") || endTurnPending}
            style={styles.ruleButton}
          >
            {turnPhase === "attack" ? "Skip Attack / End Turn" : "End Turn"}
          </button>
        </div>
        <span style={styles.ruleHint}>
          {turnPhase === "attack" && selectedUnitId
            ? attackTargetCount
              ? `${attackTargetCount} valid target(s): click a red highlighted enemy to attack for 1 damage.`
              : "No adjacent enemies on the same level. Select another friendly unit or skip attack."
            : ruleHint}
          {" "}Right-click the board or press Escape to cancel selection.
        </span>
      </section>
    </>
  );
}

const styles = {
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
    flexWrap: "wrap" as const,
  },
  modeButton: {
    borderWidth: 1,
    borderStyle: "solid" as const,
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
  headerStatus: {
    display: "flex",
    alignItems: "center",
    justifyContent: "flex-end",
    gap: 18,
    flexWrap: "wrap" as const,
  },
  kicker: { margin: "0 0 8px", color: "#59736a", fontSize: 11, fontWeight: 700, letterSpacing: 1.5 },
  title: { margin: 0, fontSize: 28, fontWeight: 650, lineHeight: 1.1 },
  connection: { display: "flex", alignItems: "center", gap: 9, color: "#3f5a51", fontSize: 13, textTransform: "capitalize" as const },
  statusDot: { width: 9, height: 9, borderRadius: "50%", display: "inline-block" },
  matchId: { borderLeft: "1px solid #bcc9bf", paddingLeft: 12, fontFamily: "ui-monospace, monospace", color: "#75847c" },
  recoveryBanner: {
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
  },
  reconnectButton: {
    padding: "8px 12px",
    border: 0,
    borderRadius: 6,
    background: "#b45309",
    color: "#fff",
    cursor: "pointer",
    fontWeight: 700,
  },
  dismissButton: {
    padding: "4px 8px",
    border: 0,
    borderRadius: 6,
    background: "transparent",
    color: "#7c2d12",
    cursor: "pointer",
    fontSize: 18,
    fontWeight: 700,
  },
  toolbar: { maxWidth: 1240, margin: "0 auto", minHeight: 52, display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap" as const, gap: 12 },
  notice: { color: "#416158", fontSize: 13, overflowWrap: "anywhere" as const },
  counts: { color: "#587168", fontSize: 12, whiteSpace: "nowrap" as const },
  rulesBar: {
    maxWidth: 1240,
    margin: "0 auto 12px",
    minHeight: 48,
    padding: "8px 12px",
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    flexWrap: "wrap" as const,
    gap: 10,
    border: "1px solid #c4d0c7",
    borderRadius: 6,
    background: "#f7faf5",
    color: "#18302d",
    fontSize: 13,
  },
  playerIdentity: {
    display: "inline-flex",
    alignItems: "center",
    gap: 8,
    padding: "7px 10px",
    border: "1px solid",
    borderRadius: 999,
    background: "#f7faf5",
    color: "#18302d",
    fontSize: 12,
    fontWeight: 700,
    whiteSpace: "nowrap" as const,
  },
  playerColorDot: { width: 10, height: 10, borderRadius: "50%", flexShrink: 0 },
  turnBanner: {
    display: "flex",
    flexDirection: "column" as const,
    gap: 4,
    flex: "1 1 240px",
    padding: "12px 16px",
    borderRadius: 6,
    color: "#fff",
    fontSize: 20,
  },
  turnDetails: { fontSize: 12, fontWeight: 400 },
  rulesButtons: { display: "flex", flexWrap: "wrap" as const, gap: 8 },
  ruleButton: {
    padding: "7px 10px",
    border: "1px solid #789087",
    borderRadius: 4,
    background: "#fff",
    color: "#18302d",
    font: "inherit",
    cursor: "pointer",
  },
  ruleHint: { flexBasis: "100%", color: "#416158" },
};
