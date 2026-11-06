import { summarizeProfiles } from "./model.js";

export const MAX_SETUP_FILE_BYTES = 1_048_576;

export function readSetupFile(file: Pick<File, "size" | "text">): Promise<string> {
  if (file.size > MAX_SETUP_FILE_BYTES) {
    throw new Error("invalid setup: file exceeds the 1 MiB setup limit (1048576 bytes)");
  }
  return file.text();
}

export function setupFileName(name: string): string {
  const sanitized = name.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 128);
  return `${sanitized || "setup"}.setup.json`;
}

/** Display metadata only, read after the core has accepted the setup or saved record. */
export function loadedProfilesFromSetup(record: string) {
  return summarizeProfiles(JSON.parse(record).profiles);
}

export async function loadSetupFile(
  file: Pick<File, "size" | "text">,
  confirmReplace: () => boolean,
  apply: (text: string) => void,
  refreshAndSave: () => void,
): Promise<boolean> {
  const text = await readSetupFile(file);
  if (!confirmReplace()) return false;
  apply(text);
  refreshAndSave();
  return true;
}
