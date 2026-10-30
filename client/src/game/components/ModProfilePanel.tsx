import type { ModProfileSummary, PlayerSeat, RenderEntity } from "../types";

type ModProfilePanelProps = {
  profileText: string;
  onProfileTextChange: (value: string) => void;
  enabled: boolean;
  onLoadProfiles: () => void;
  loadedProfiles: ModProfileSummary[];
  selectedProfile: ModProfileSummary | null;
  onSelectProfile: (modId: string) => void;
  deploymentSeat: PlayerSeat;
  onDeploymentSeatChange: (seat: PlayerSeat) => void;
  deploymentSlots: RenderEntity[];
  deploymentSlotId: string;
  onDeploymentSlotChange: (slotId: string) => void;
  startingHealth: string;
  onStartingHealthChange: (value: string) => void;
  onDeployProfile: () => void;
  setupLocked: boolean;
  matchFinished: boolean;
  onStartFresh: () => void;
};

export function ModProfilePanel({
  profileText,
  onProfileTextChange,
  enabled,
  onLoadProfiles,
  loadedProfiles,
  selectedProfile,
  onSelectProfile,
  deploymentSeat,
  onDeploymentSeatChange,
  deploymentSlots,
  deploymentSlotId,
  onDeploymentSlotChange,
  startingHealth,
  onStartingHealthChange,
  onDeployProfile,
  setupLocked,
  matchFinished,
  onStartFresh,
}: ModProfilePanelProps) {
  return (
    <details style={styles.section}>
      <summary style={styles.summary}>🔌 Community Mod Profile Loader</summary>
      <label style={styles.label} htmlFor="mod-profile-json">
        Paste a mod profile or profile batch as JSON
      </label>
      <textarea
        id="mod-profile-json"
        aria-label="Community mod profile JSON"
        value={profileText}
        onChange={(event) => onProfileTextChange(event.target.value)}
        placeholder={'{\n  "schema_version": 1,\n  "mod_id": "com.community.scout",\n  "display_name": "Community Scout",\n  "visual_primitive": "CUBE",\n  "components": {\n    "destructible": { "current_life": 5, "max_life": 5 },\n    "tags": ["Unit"]\n  }\n}'}
        style={styles.input}
      />
      <button
        type="button"
        onClick={onLoadProfiles}
        disabled={!enabled}
        style={styles.button}
      >
        Load Mod Profile
      </button>
      {loadedProfiles.length > 0 && (
        <div style={{ marginTop: 12 }}>
          <label style={styles.label} htmlFor="profile-to-deploy">
            Unit profile
          </label>
          <select
            id="profile-to-deploy"
            aria-label="Unit profile to deploy"
            value={selectedProfile?.modId ?? ""}
            onChange={(event) => onSelectProfile(event.target.value)}
            style={styles.networkInput}
          >
            {loadedProfiles.map((profile) => (
              <option key={profile.modId} value={profile.modId}>
                {profile.displayName} · {profile.modId}
              </option>
            ))}
          </select>
          <label style={styles.label} htmlFor="profile-deployment-seat">
            Player seat
          </label>
          <select
            id="profile-deployment-seat"
            aria-label="Player seat for profile deployment"
            value={deploymentSeat}
            onChange={(event) => {
              const seat = event.target.value;
              if (seat === "P1" || seat === "P2") onDeploymentSeatChange(seat);
            }}
            style={styles.networkInput}
          >
            <option value="P1">P1</option>
            <option value="P2">P2</option>
          </select>
          <label style={styles.label} htmlFor="profile-deployment-slot">
            Roster slot to replace
          </label>
          <select
            id="profile-deployment-slot"
            aria-label="Roster slot to replace"
            value={deploymentSlotId}
            onChange={(event) => onDeploymentSlotChange(event.target.value)}
            disabled={deploymentSlots.length === 0}
            style={styles.networkInput}
          >
            {deploymentSlots.map((unit) => (
              <option key={unit.id} value={unit.id}>
                {unit.id}{unit.display_name ? ` · ${unit.display_name}` : ""}
              </option>
            ))}
          </select>
          <label style={styles.label} htmlFor="profile-starting-health">
            Starting health (max {selectedProfile?.maxHealth ?? "—"})
          </label>
          <input
            id="profile-starting-health"
            aria-label="Starting health"
            type="number"
            min={1}
            max={selectedProfile?.maxHealth}
            step={1}
            value={startingHealth}
            onChange={(event) => onStartingHealthChange(event.target.value)}
            style={styles.networkInput}
          />
          <button
            type="button"
            onClick={onDeployProfile}
            disabled={
              !enabled ||
              !selectedProfile ||
              deploymentSlots.length === 0 ||
              setupLocked ||
              matchFinished
            }
            style={styles.button}
          >
            Deploy Profile into Roster Slot
          </button>
          <p role="note" style={{ margin: "6px 0 0", fontSize: 12 }}>
            The Rust core replaces the selected unit in place and owns its health, tags, and visual. Deployment locks after the first referee action.
          </p>
        </div>
      )}
      {loadedProfiles.length > 0 && (
        <ul style={styles.profileList} aria-label="Loaded mod profiles">
          {loadedProfiles.map((profile) => (
            <li key={profile.modId}>
              {profile.displayName} · {profile.modId} · {profile.visualPrimitive}
            </li>
          ))}
        </ul>
      )}
      <button
        type="button"
        onClick={onStartFresh}
        disabled={!enabled}
        style={styles.button}
      >
        Start Fresh Sandbox
      </button>
      <p role="note" style={{ margin: "6px 0 0", fontSize: 12 }}>
        This tab keeps the local sandbox and its profiles across a refresh. Start fresh discards them.
      </p>
    </details>
  );
}

const styles = {
  section: { marginTop: 14, paddingTop: 12, borderTop: "1px solid #4d6860" },
  summary: { cursor: "pointer", fontSize: 12, fontWeight: 650, lineHeight: 1.5 },
  label: { display: "block", margin: "10px 0 5px", color: "#b4c5ba", fontSize: 11 },
  input: {
    width: "100%",
    minHeight: 150,
    boxSizing: "border-box" as const,
    resize: "vertical" as const,
    padding: 8,
    border: "1px solid #4d6860",
    background: "#101c1d",
    color: "#e5efe8",
    font: "11px/1.45 ui-monospace, monospace",
  },
  button: {
    marginTop: 8,
    padding: "7px 10px",
    border: "1px solid #70d6a4",
    background: "#183b35",
    color: "#f1f4ed",
    font: "inherit",
    fontSize: 11,
    cursor: "pointer",
  },
  networkInput: {
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
  profileList: {
    margin: "9px 0 0",
    paddingLeft: 16,
    color: "#b4c5ba",
    fontSize: 10,
    overflowWrap: "anywhere" as const,
  },
};
