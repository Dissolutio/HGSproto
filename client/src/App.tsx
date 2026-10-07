import { useState } from "react";
import "./App.css";
import { calculateHexDistance } from "./services/engine";

function App() {
  const [output, setOutput] = useState<string>('Click button to evaluate distance...');

  const handleTestTrigger = async () => {
    // Call the wrapper. The engine.ts file figures out the platform physics automatically.
    const result = await calculateHexDistance(0, 1, 2, 3);
    setOutput(result);
  };

  return (
    <div style={{ padding: '20px', textAlign: 'center' }}>
      <h3>HexGameSim MVP Engine Bridge Checkpoint</h3>
      <button onClick={handleTestTrigger} style={{ padding: '10px 20px', cursor: 'pointer' }}>
        Calculate Hex Distance
      </button>
      <p style={{ marginTop: '15px', fontWeight: 'bold', color: '#4caf50' }}>{output}</p>
    </div>
  );
}

export default App;
