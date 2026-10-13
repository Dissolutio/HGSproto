import { formatNodeId } from "../constants";
import type { ConnectionState, Mode } from "../types";

type NetworkPortalProps = {
  mode: Mode;
  connection: ConnectionState;
  localNodeId: string | null;
  localMatchId: string | null;
  remoteNodeId: string | null;
  joinNodeId: string;
  onJoinNodeIdChange: (value: string) => void;
  onHost: () => void;
  onJoin: () => void;
  onDisconnectAndReset: () => void;
  socketReady: boolean;
};

export function NetworkPortal({
  mode,
  connection,
  localNodeId,
  localMatchId,
  remoteNodeId,
  joinNodeId,
  onJoinNodeIdChange,
  onHost,
  onJoin,
  onDisconnectAndReset,
  socketReady,
}: NetworkPortalProps) {
  return (
    <details style={styles.section}>
      <summary style={styles.summary}>🌐 Multiplayer Connection Portal</summary>
      <p style={styles.help}>
        Host or join through the local native gateway. Iroh handles encrypted peer transport and relay discovery; browser traffic reaches it over this local WebSocket. If it is unavailable, start it with "cargo run -p hex_game_orchestrator" from the repository root.
      </p>
      <table style={styles.statusTable}>
        <tbody>
          <tr>
            <th style={styles.statusLabel}>Your Node ID:</th>
            <td style={styles.statusValue}>
              {localNodeId
                ? formatNodeId(localNodeId)
                : <>None <em>(Click Host or Join to allocate key)</em></>}
            </td>
          </tr>
          <tr>
            <th style={styles.statusLabel}>Opponent Node ID:</th>
            <td style={styles.statusValue}>
              {remoteNodeId ? formatNodeId(remoteNodeId) : "Waiting for peer..."}
            </td>
          </tr>
          <tr>
            <th style={styles.statusLabel}>Active Connection Transport:</th>
            <td style={styles.statusValue}>
              {mode === "network" && connection === "connected"
                ? "Local Gateway WebSocket (Port 8080)"
                : "WASM Sandbox"}
            </td>
          </tr>
        </tbody>
      </table>
      <button
        type="button"
        onClick={onHost}
        disabled={mode !== "network" || !socketReady || !!localNodeId}
        style={styles.button}
      >
        Host Match
      </button>
      {localNodeId && localMatchId && (
        <label style={styles.label} htmlFor="local-iroh-node-id">
          Your connection ticket (copy for the other tab)
          <input
            id="local-iroh-node-id"
            readOnly
            value={`${localNodeId}@${localMatchId}`}
            onFocus={(event) => event.currentTarget.select()}
            style={styles.input}
          />
        </label>
      )}
      <label style={styles.label} htmlFor="remote-iroh-node-id">
        Host connection ticket
        <input
          id="remote-iroh-node-id"
          value={joinNodeId}
          onChange={(event) => onJoinNodeIdChange(event.target.value)}
          placeholder="Node ID@match ID"
          style={styles.input}
        />
      </label>
      <button
        type="button"
        onClick={onJoin}
        disabled={mode !== "network" || !socketReady}
        style={styles.button}
      >
        Join Match
      </button>
      <button
        type="button"
        onClick={onDisconnectAndReset}
        style={styles.resetButton}
      >
        🛑 Leave Match &amp; Reset Gateway
      </button>
    </details>
  );
}

const styles = {
  section: { marginTop: 14, paddingTop: 12, borderTop: "1px solid #4d6860" },
  summary: { cursor: "pointer", fontSize: 12, fontWeight: 650, lineHeight: 1.5 },
  help: { margin: "8px 0", color: "#9aac9e", fontSize: 10, lineHeight: 1.45 },
  statusTable: {
    width: "100%",
    margin: "8px 0",
    borderCollapse: "collapse" as const,
    fontSize: 10,
  },
  statusLabel: {
    padding: "4px 8px 4px 0",
    color: "#9aac9e",
    fontWeight: 500,
    textAlign: "left" as const,
    verticalAlign: "top" as const,
  },
  statusValue: {
    padding: "4px 0",
    color: "#e5efe8",
    overflowWrap: "anywhere" as const,
    textAlign: "left" as const,
  },
  label: { display: "block", margin: "10px 0 5px", color: "#b4c5ba", fontSize: 11 },
  input: {
    display: "block",
    width: "100%",
    boxSizing: "border-box" as const,
    marginTop: 5,
    padding: 7,
    border: "1px solid #4d6860",
    background: "#101c1d",
    color: "#e5efe8",
    font: "10px ui-monospace, monospace",
  },
  button: {
    margin: "4px 5px 4px 0",
    padding: "7px 10px",
    border: "1px solid #70d6a4",
    background: "#183b35",
    color: "#f1f4ed",
    font: "inherit",
    fontSize: 11,
    cursor: "pointer",
  },
  resetButton: {
    marginTop: 12,
    width: "100%",
    padding: 6,
    background: "#dc2626",
    color: "#fff",
    border: "none",
    borderRadius: 4,
    cursor: "pointer",
    fontWeight: "bold",
  },
};
