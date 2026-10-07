import { useState } from "react";
import "./App.css";
import { calculateHexDistance } from "./services/engine";
import { loadUserData, saveUserData } from "./services/storage";

function App() {
  const [output, setOutput] = useState<string>('Click button to evaluate distance...');
  const [storageOutput, setStorageOutput] = useState<string>('Storage test has not run yet.');
  const [isTestingStorage, setIsTestingStorage] = useState(false);

  const handleTestTrigger = async () => {
    // Call the wrapper. The engine.ts file figures out the platform physics automatically.
    const result = await calculateHexDistance(0, 1, 2, 3);
    setOutput(result);
  };

  const handleStorageTest = async () => {
    setIsTestingStorage(true);
    setStorageOutput('Saving and loading test data...');

    try {
      const key = 'hexgamesim-storage-test';
      const testData = {
        message: 'Storage round-trip works',
        timestamp: new Date().toISOString(),
      };

      await saveUserData(key, testData);
      const loadedData = await loadUserData(key);
      const passed = JSON.stringify(loadedData) === JSON.stringify(testData);

      setStorageOutput(
        passed
          ? `PASS: saved and loaded ${JSON.stringify(loadedData)}`
          : `FAIL: expected ${JSON.stringify(testData)}, received ${JSON.stringify(loadedData)}`,
      );
    } catch (error) {
      setStorageOutput(`ERROR: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setIsTestingStorage(false);
    }
  };

  return (
    <div style={{ padding: '20px', textAlign: 'center' }}>
      <h3>HexGameSim MVP Engine Bridge Checkpoint</h3>
      <button onClick={handleTestTrigger} style={{ padding: '10px 20px', cursor: 'pointer' }}>
        Calculate Hex Distance
      </button>
      <p style={{ marginTop: '15px', fontWeight: 'bold', color: '#4caf50' }}>{output}</p>
      <hr />
      <h3>Cross-Platform Storage Check</h3>
      <button onClick={handleStorageTest} disabled={isTestingStorage}>
        {isTestingStorage ? 'Testing Storage...' : 'Test Save / Load'}
      </button>
      <p aria-live="polite">{storageOutput}</p>
    </div>
  );
}

export default App;
