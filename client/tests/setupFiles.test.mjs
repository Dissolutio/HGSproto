import assert from "node:assert/strict";
import test from "node:test";
import {
  loadedProfilesFromSetup,
  loadSetupFile,
  MAX_SETUP_FILE_BYTES,
  readSetupFile,
  setupFileName,
} from "../.test-build/game/setupFiles.js";

test("exactly 1 MiB is read unchanged; one byte over is refused before any read", async () => {
  let reads = 0;
  const file = {
    size: MAX_SETUP_FILE_BYTES,
    text: async () => { reads++; return "opaque setup text"; },
  };
  assert.equal(await readSetupFile(file), "opaque setup text");
  assert.equal(reads, 1);
  file.size++;
  assert.throws(() => readSetupFile(file), {
    message: "invalid setup: file exceeds the 1 MiB setup limit (1048576 bytes)",
  });
  assert.equal(reads, 1);
});

test("setup filenames keep only ASCII letters, digits, dash and underscore, with a fallback and bound", () => {
  for (const [name, expected] of [
    ["", "setup.setup.json"],
    ["日本語💥", "setup.setup.json"],
    ["../a\\b/c.json", "abcjson.setup.json"],
    ["Goliath skirmish_2-v1", "Goliathskirmish_2-v1.setup.json"],
    ["x".repeat(10_000), `${"x".repeat(128)}.setup.json`],
  ]) {
    const filename = setupFileName(name);
    assert.equal(filename, expected);
    assert.match(filename, /^[A-Za-z0-9_-]+\.setup\.json$/);
  }
});

test("loaded profiles are rebuilt from accepted setup or saved record, including tagged core components", () => {
  const profile = {
    mod_id: "com.community.goliath",
    display_name: "Goliath",
    visual_primitive: "CUBE",
    components: { destructible: { current_life: 12, max_life: 12 } },
  };
  const expected = [{
    modId: profile.mod_id, displayName: "Goliath", visualPrimitive: "CUBE",
    currentHealth: 12, maxHealth: 12,
  }];
  assert.deepEqual(loadedProfilesFromSetup(JSON.stringify({ profiles: [profile] })), expected);
  const tagged = { ...profile, components: {
    destructible: { type: "Destructible", data: { current_life: 12, max_life: 12 } },
  } };
  assert.deepEqual(loadedProfilesFromSetup(JSON.stringify({
    record_version: 2, profiles: [tagged], setup: null,
  })), expected);
  assert.deepEqual(loadedProfilesFromSetup('{"profiles":[]}'), []);
});

test("load order is size guard, read, confirmation, unchanged core input, refresh and save", async () => {
  const calls = [];
  const text = ' { "unknown": "left for the core" } ';
  await loadSetupFile(
    { size: 40, text: async () => { calls.push("read"); return text; } },
    () => { calls.push("confirm"); return true; },
    (received) => { assert.equal(received, text); calls.push("apply"); },
    () => calls.push("refresh and save"),
  );
  assert.deepEqual(calls, ["read", "confirm", "apply", "refresh and save"]);
});

test("cancelled or refused setup never refreshes UI or saves, and preserves the exact core error", async () => {
  let screen = "old screen";
  let saved = "old saved record";
  let applies = 0;
  const file = { size: 2, text: async () => "{}" };
  const refreshAndSave = () => { screen = "new"; saved = "new"; };
  assert.equal(await loadSetupFile(file, () => false, () => applies++, refreshAndSave), false);
  assert.equal(applies, 0);
  const reason = "invalid setup: unknown field `rules`\ncore detail";
  await assert.rejects(loadSetupFile(file, () => true, () => { throw reason; }, refreshAndSave),
    (error) => error === reason);
  await assert.rejects(loadSetupFile(
    { ...file, size: MAX_SETUP_FILE_BYTES + 1 },
    () => assert.fail("no confirmation"), () => assert.fail("no apply"), refreshAndSave,
  ), /1 MiB/);
  assert.equal(screen, "old screen");
  assert.equal(saved, "old saved record");
});

test("a read failure does not confirm, apply, refresh or save", async () => {
  const reason = new Error("file read failed");
  const unexpected = () => assert.fail("must not be called");
  await assert.rejects(loadSetupFile(
    { size: 1, text: async () => { throw reason; } }, unexpected, unexpected, unexpected,
  ), (error) => error === reason);
});
