import assert from "node:assert/strict";
import test from "node:test";
import {
  discardSavedSandbox,
  LOCAL_SANDBOX_STORAGE_KEY,
  openSandboxStorage,
  REJECTED_LOCAL_SANDBOX_STORAGE_KEY,
  restoreSavedSandbox,
  saveSandbox,
} from "../.test-build/game/sandboxPersistence.js";

class MemoryStorage {
  constructor(entries = {}) {
    this.items = new Map(Object.entries(entries));
    this.failSet = null;
    this.failRemove = false;
  }
  getItem(key) {
    return this.items.has(key) ? this.items.get(key) : null;
  }
  setItem(key, value) {
    if (this.failSet) throw new Error(this.failSet);
    this.items.set(key, String(value));
  }
  removeItem(key) {
    if (this.failRemove) throw new Error("remove blocked");
    this.items.delete(key);
  }
}

const throwingRestore = (message) => () => {
  throw message;
};

test("storage unavailable starts fresh without saving and without crashing", () => {
  const storage = openSandboxStorage(() => {
    throw new Error("SecurityError: storage is blocked");
  });
  assert.equal(storage, null);
  let restoreCalls = 0;
  const startup = restoreSavedSandbox(storage, () => restoreCalls++);
  assert.equal(startup.kind, "unavailable");
  assert.equal(startup.savingEnabled, false);
  assert.match(startup.message, /will not survive a refresh/);
  assert.equal(restoreCalls, 0);
  assert.match(saveSandbox(storage, () => "{}"), /not saved/);
  assert.equal(discardSavedSandbox(storage), null);
});

test("storage that throws on read is treated as unavailable", () => {
  const storage = new MemoryStorage();
  storage.getItem = () => {
    throw new Error("read blocked");
  };
  assert.equal(openSandboxStorage(() => storage), null);
  assert.equal(restoreSavedSandbox(storage, () => 0).kind, "unavailable");
});

test("no saved record starts a fresh sandbox with saving on", () => {
  const startup = restoreSavedSandbox(new MemoryStorage(), () => assert.fail("no restore"));
  assert.deepEqual(startup, { kind: "empty", savingEnabled: true });
});

test("a saved record is handed to the core restore unchanged", () => {
  const storage = new MemoryStorage({ [LOCAL_SANDBOX_STORAGE_KEY]: "RECORD" });
  let received = null;
  const startup = restoreSavedSandbox(storage, (record) => {
    received = record;
    return 2;
  });
  assert.equal(received, "RECORD");
  assert.deepEqual(startup, {
    kind: "restored",
    savingEnabled: true,
    record: "RECORD",
    profileCount: 2,
  });
  assert.equal(storage.getItem(LOCAL_SANDBOX_STORAGE_KEY), "RECORD");
});

test("a refused record (corrupt, wrong version, unknown profile, missing snapshot) is kept aside", () => {
  for (const reason of [
    "Saved sandbox record is not valid JSON",
    "Unsupported saved sandbox record_version 2; expected 1",
    "Saved snapshot references profile 'com.x.y' that is not in the saved profiles",
    "Invalid saved sandbox record: missing field `snapshot`",
  ]) {
    const storage = new MemoryStorage({ [LOCAL_SANDBOX_STORAGE_KEY]: "BAD" });
    const startup = restoreSavedSandbox(storage, throwingRestore(reason));
    assert.equal(startup.kind, "rejected");
    assert.equal(startup.savingEnabled, true);
    assert.ok(startup.message.includes(reason));
    assert.match(startup.message, /kept aside/);
    assert.equal(storage.getItem(LOCAL_SANDBOX_STORAGE_KEY), null);
    assert.equal(storage.getItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY), "BAD");
  }
});

test("a refused record is never overwritten when it cannot be moved aside", () => {
  const occupied = new MemoryStorage({
    [LOCAL_SANDBOX_STORAGE_KEY]: "NEW_BAD",
    [REJECTED_LOCAL_SANDBOX_STORAGE_KEY]: "OLD_BAD",
  });
  const startup = restoreSavedSandbox(occupied, throwingRestore("corrupt"));
  assert.equal(startup.kind, "rejected");
  assert.equal(startup.savingEnabled, false);
  assert.match(startup.message, /saving is paused/);
  assert.equal(occupied.getItem(LOCAL_SANDBOX_STORAGE_KEY), "NEW_BAD");
  assert.equal(occupied.getItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY), "OLD_BAD");

  const full = new MemoryStorage({ [LOCAL_SANDBOX_STORAGE_KEY]: "BAD" });
  full.failSet = "QuotaExceededError";
  const fullStartup = restoreSavedSandbox(full, throwingRestore(new Error("corrupt")));
  assert.equal(fullStartup.savingEnabled, false);
  assert.equal(full.getItem(LOCAL_SANDBOX_STORAGE_KEY), "BAD");

  const stuck = new MemoryStorage({ [LOCAL_SANDBOX_STORAGE_KEY]: "BAD" });
  stuck.failRemove = true;
  const stuckStartup = restoreSavedSandbox(stuck, throwingRestore("corrupt"));
  assert.equal(stuckStartup.savingEnabled, false);
  assert.equal(stuck.getItem(LOCAL_SANDBOX_STORAGE_KEY), "BAD");
  assert.equal(stuck.getItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY), "BAD");
});

test("saving writes the core record and reports a full or failing store", () => {
  const storage = new MemoryStorage({ [LOCAL_SANDBOX_STORAGE_KEY]: "OLD" });
  assert.equal(saveSandbox(storage, () => "NEW"), null);
  assert.equal(storage.getItem(LOCAL_SANDBOX_STORAGE_KEY), "NEW");

  storage.failSet = "QuotaExceededError";
  assert.match(saveSandbox(storage, () => "NEWER"), /not saved \(QuotaExceededError\)/);
  assert.equal(storage.getItem(LOCAL_SANDBOX_STORAGE_KEY), "NEW");

  storage.failSet = null;
  assert.match(saveSandbox(storage, throwingRestore("Unknown match")), /not saved \(Unknown match\)/);
  assert.equal(storage.getItem(LOCAL_SANDBOX_STORAGE_KEY), "NEW");
});

test("start fresh discards both the saved and the kept-aside record", () => {
  const storage = new MemoryStorage({
    [LOCAL_SANDBOX_STORAGE_KEY]: "GOOD",
    [REJECTED_LOCAL_SANDBOX_STORAGE_KEY]: "BAD",
    "hexgamesim.tab-id": "tab",
  });
  assert.equal(discardSavedSandbox(storage), null);
  assert.equal(storage.getItem(LOCAL_SANDBOX_STORAGE_KEY), null);
  assert.equal(storage.getItem(REJECTED_LOCAL_SANDBOX_STORAGE_KEY), null);
  assert.equal(storage.getItem("hexgamesim.tab-id"), "tab");

  storage.failRemove = true;
  assert.match(discardSavedSandbox(storage), /Could not discard/);
});
