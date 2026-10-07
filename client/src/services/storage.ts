// client/src/services/storage.ts
import { Store } from '@tauri-apps/plugin-store';

let tauriStore: Store | null = null;
const isNativeApp = typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;

// Helper function to safely load or return the store instance
async function getTauriStore(): Promise<Store | null> {
  if (!isNativeApp) return null;
  // If already initialized, return it immediately
  if (tauriStore) return tauriStore;

  // Highlight-start
  // Securely initialize using the v2 static loader method
  tauriStore = await Store.load('.hexgamesim-settings.dat');
  // Highlight-end
  return tauriStore;
}

/**
 * Universal Cross-Platform Save Hook
 */
export async function saveUserData(key: string, value: any): Promise<void> {
  const store = await getTauriStore();

  if (store) {
    // 📱 Native App Mode: Write to secure local hard disk via Rust
    await store.set(key, value);
    await store.save(); // Force Rust to flush the buffer to disk
  } else {
    // 🌐 Web Browser Mode: Fallback to sandboxed localStorage
    localStorage.setItem(key, JSON.stringify(value));
  }
}

/**
 * Universal Cross-Platform Load Hook
 */
export async function loadUserData(key: string): Promise<any | null> {
  const store = await getTauriStore();

  if (store) {
    return await store.get(key);
  } else {
    const data = localStorage.getItem(key);
    return data ? JSON.parse(data) : null;
  }
}
