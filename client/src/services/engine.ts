// client/src/services/engine.ts
import { invoke } from '@tauri-apps/api/core';

/**
 * Unified Cross-Platform Execution Bridge
 * Contributors call this function anywhere in the React layout.
 */
export async function calculateHexDistance(q1: number, r1: number, q2: number, r2: number): Promise<string> {
  // Check if we are running inside the native Tauri container (Android or Desktop)
  const isNativeApp = typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;

  if (isNativeApp) {
    try {
      // 📱 Mobile / Desktop Mode: Fire across the live native system bridge channel
      return await invoke('calculate_hex_distance', { q1, r1, q2, r2 });
    } catch (error) {
      return `Native Engine Bridge Error: ${error}`;
    }
  } else {
    // 🌐 Web Browser Mode Fallback: Return a clean placeholder mock until WASM builds are generated
    return `[Mock Browser Engine] Distance between (${q1},${r1}) and (${q2},${r2}) is calculated as 3 tiles.`;
  }
}
