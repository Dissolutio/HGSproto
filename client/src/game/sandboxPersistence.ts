export const LOCAL_SANDBOX_STORAGE_KEY = "hexgamesim.local-sandbox";
export const REJECTED_LOCAL_SANDBOX_STORAGE_KEY = "hexgamesim.local-sandbox.rejected";

export type SandboxStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export type SandboxStartup =
  | { kind: "unavailable"; savingEnabled: false; message: string }
  | { kind: "empty"; savingEnabled: true }
  | { kind: "restored"; savingEnabled: true; record: string; profileCount: number }
  | { kind: "rejected"; savingEnabled: boolean; message: string };

const UNAVAILABLE_MESSAGE =
  "Browser storage is unavailable; this sandbox will not survive a refresh";

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** Returns the storage only if it can be reached; blocked storage throws on access. */
export function openSandboxStorage(getStorage: () => SandboxStorage): SandboxStorage | null {
  try {
    const storage = getStorage();
    storage.getItem(LOCAL_SANDBOX_STORAGE_KEY);
    return storage;
  } catch {
    return null;
  }
}

/**
 * Hands a saved record to the core restore. A refused record is moved aside unchanged;
 * if that is not possible it is left in place and saving is paused so it is never overwritten.
 */
export function restoreSavedSandbox(
  storage: SandboxStorage | null,
  restore: (record: string) => number,
): SandboxStartup {
  if (!storage) return { kind: "unavailable", savingEnabled: false, message: UNAVAILABLE_MESSAGE };
  let record: string | null;
  try {
    record = storage.getItem(LOCAL_SANDBOX_STORAGE_KEY);
  } catch {
    return { kind: "unavailable", savingEnabled: false, message: UNAVAILABLE_MESSAGE };
  }
  if (record === null) return { kind: "empty", savingEnabled: true };

  let reason: string;
  try {
    const profileCount = restore(record);
    return { kind: "restored", savingEnabled: true, record, profileCount };
  } catch (error) {
    reason = errorText(error);
  }

  let setAside = false;
  try {
    if (storage.getItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY) === null) {
      storage.setItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY, record);
      storage.removeItem(LOCAL_SANDBOX_STORAGE_KEY);
      setAside = true;
    }
  } catch {
    setAside = false;
  }
  return {
    kind: "rejected",
    savingEnabled: setAside,
    message: setAside
      ? `Saved sandbox could not be restored (${reason}). A fresh sandbox was started; the saved copy was kept aside. Use Start fresh to discard it.`
      : `Saved sandbox could not be restored (${reason}). A fresh sandbox was started; the saved copy was left in place and saving is paused. Use Start fresh to discard it.`,
  };
}

/** Saves the core-exported record. Returns a warning when the change was not saved. */
export function saveSandbox(
  storage: SandboxStorage | null,
  exportRecord: () => string,
): string | null {
  if (!storage) return "not saved: browser storage is unavailable";
  try {
    storage.setItem(LOCAL_SANDBOX_STORAGE_KEY, exportRecord());
    return null;
  } catch (error) {
    return `not saved (${errorText(error)}); a refresh will return to the last saved point`;
  }
}

/** Removes the saved and the set-aside record. Returns an error message on failure. */
export function discardSavedSandbox(storage: SandboxStorage | null): string | null {
  if (!storage) return null;
  try {
    storage.removeItem(LOCAL_SANDBOX_STORAGE_KEY);
    storage.removeItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY);
    return null;
  } catch (error) {
    return `Could not discard the saved sandbox: ${errorText(error)}`;
  }
}
