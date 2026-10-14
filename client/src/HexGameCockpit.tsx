import { useGameRuntime } from "./game/hooks/useGameRuntime";
import { GameHud } from "./game/components/GameHud";
import { HexBoard } from "./game/components/HexBoard";
import { InspectorPanel } from "./game/components/InspectorPanel";
import { ModProfilePanel } from "./game/components/ModProfilePanel";
import { NetworkPortal } from "./game/components/NetworkPortal";
import { PLAYER_COLORS } from "./game/constants";

export default function HexGameCockpit() {
  const {
    state: {
      mode, gameState, selectedUnitId, actionPending, endTurnAfterSkip, inspectedEntity,
      connection, notice, socket, localNodeId, localMatchId, remoteNodeId,
      isPeerConnected, joinNodeId, setJoinNodeId, recoverySession, setRecoveryDismissed,
    },
    view: {
      terrain, units, deploymentSlots, deploymentSlot, selectedProfile, activeSeat,
      turnPhase, winner, playerSeat, isYourTurn, canAct, attackTargetIds,
      playerOneCount, playerTwoCount, ruleHint, showRecoveryBanner, healthLabelElements,
    },
    profiles: {
      modProfileText, setModProfileText, loadedModProfiles, setSelectedModId,
      deploymentSeat, setDeploymentSeat, setDeploymentSlotId, startingHealth,
      setStartingHealth, loadModProfiles, handleDeployProfile,
    },
    actions: {
      chooseMode, clearSelection, submitAction, handleEndTurn, selectUnit, handleHexClick,
      handleGracefulReconnection, handleHostMatch, handleJoinMatch, handleDisconnectAndReset,
    },
  } = useGameRuntime();

  return (
    <main style={styles.page}>
      <GameHud
        mode={mode}
        onModeChange={chooseMode}
        connection={connection}
        matchId={gameState?.context?.match_id}
        isPeerConnected={isPeerConnected}
        recoverySession={recoverySession}
        showRecoveryBanner={showRecoveryBanner}
        onReconnect={handleGracefulReconnection}
        onDismissRecovery={() => setRecoveryDismissed(true)}
        notice={notice}
        terrainCount={terrain.length}
        unitCount={units.length}
        playerSeat={playerSeat}
        activeSeat={activeSeat}
        turnPhase={turnPhase}
        winner={winner}
        hasGameState={!!gameState}
        isYourTurn={isYourTurn}
        actionPending={actionPending}
        canAct={canAct}
        selectedUnitId={selectedUnitId}
        attackTargetCount={attackTargetIds.size}
        ruleHint={ruleHint}
        endTurnPending={!!endTurnAfterSkip}
        onClearSelection={clearSelection}
        onSkipMovement={() => submitAction("S")}
        onEndTurn={handleEndTurn}
      />

      <HexBoard
        terrain={terrain}
        units={units}
        selectedUnitId={selectedUnitId}
        attackTargetIds={attackTargetIds}
        healthLabelElements={healthLabelElements}
        onSelectUnit={selectUnit}
        onHexClick={handleHexClick}
        onContextMenu={(event) => {
          event.preventDefault();
          clearSelection();
        }}
      >
        <InspectorPanel entity={inspectedEntity}>
          <ModProfilePanel
            profileText={modProfileText}
            onProfileTextChange={setModProfileText}
            enabled={mode === "wasm" && connection === "ready"}
            onLoadProfiles={loadModProfiles}
            loadedProfiles={loadedModProfiles}
            selectedProfile={selectedProfile ?? null}
            onSelectProfile={(modId) => {
              const profile = loadedModProfiles.find((item) => item.modId === modId);
              setSelectedModId(modId);
              if (profile) setStartingHealth(profile.currentHealth.toString());
            }}
            deploymentSeat={deploymentSeat}
            onDeploymentSeatChange={(seat) => {
              setDeploymentSeat(seat);
              setDeploymentSlotId("");
            }}
            deploymentSlots={deploymentSlots}
            deploymentSlotId={deploymentSlot?.id ?? ""}
            onDeploymentSlotChange={setDeploymentSlotId}
            startingHealth={startingHealth}
            onStartingHealthChange={setStartingHealth}
            onDeployProfile={handleDeployProfile}
            setupLocked={!!gameState?.context?.setup_locked}
            matchFinished={!!gameState?.context?.winner}
          />
          <NetworkPortal
            mode={mode}
            connection={connection}
            localNodeId={localNodeId}
            localMatchId={localMatchId}
            remoteNodeId={remoteNodeId}
            joinNodeId={joinNodeId}
            onJoinNodeIdChange={setJoinNodeId}
            onHost={handleHostMatch}
            onJoin={handleJoinMatch}
            onDisconnectAndReset={handleDisconnectAndReset}
            socketReady={!!socket && socket.readyState === WebSocket.OPEN}
          />
        </InspectorPanel>
      </HexBoard>

      <footer style={styles.footer}>
        <span><b style={{ color: PLAYER_COLORS.P1 }}>P1</b> Player 1 · {playerOneCount}</span>
        <span><b style={{ color: PLAYER_COLORS.P2 }}>P2</b> Player 2 · {playerTwoCount}</span>
        <span style={styles.hint}>
          {selectedUnitId ? `Selected: ${selectedUnitId} · ${ruleHint}` : ruleHint}
        </span>
      </footer>
    </main>
  );
}

const styles = {
  page: {
    minHeight: "100vh",
    padding: "20px clamp(16px, 4vw, 56px) 28px",
    background: "#e9eee8",
    color: "#18302d",
    fontFamily: "'Avenir Next', Avenir, 'Segoe UI', sans-serif",
    boxSizing: "border-box" as const,
  },
  footer: {
    maxWidth: 1240,
    margin: "0 auto",
    display: "flex",
    alignItems: "center",
    gap: 22,
    paddingTop: 16,
    color: "#49635a",
    fontSize: 12,
  },
  hint: { marginLeft: "auto", textAlign: "right" as const, color: "#688077" },
};
