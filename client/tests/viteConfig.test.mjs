import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { resolveConfig } from "vite";

test("resolved dev server defaults to loopback, allows explicit LAN access, and preserves Tauri", async () => {
  const original = {
    TAURI_DEV_HOST: process.env.TAURI_DEV_HOST,
    VITE_DEV_NETWORK: process.env.VITE_DEV_NETWORK,
  };
  try {
    for (const [network, tauriHost, expectedHost] of [
      [undefined, undefined, "127.0.0.1"],
      ["0", undefined, "127.0.0.1"],
      ["true", undefined, "127.0.0.1"],
      ["1", undefined, true],
      [undefined, "192.0.2.1", "192.0.2.1"],
      ["1", "192.0.2.1", "192.0.2.1"],
    ]) {
      for (const [key, value] of Object.entries({
        VITE_DEV_NETWORK: network,
        TAURI_DEV_HOST: tauriHost,
      })) {
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      }
      const config = await resolveConfig({
        root: fileURLToPath(new URL("../", import.meta.url)),
      }, "serve");
      assert.equal(config.server.host, expectedHost);
      assert.equal(config.server.port, 1420);
      assert.equal(config.server.strictPort, true);
      assert.equal(config.server.hmr?.host, tauriHost);
      assert.equal(config.server.hmr?.protocol, tauriHost ? "ws" : undefined);
      assert.equal(config.server.hmr?.port, tauriHost ? 1421 : undefined);
    }
  } finally {
    for (const [key, value] of Object.entries(original)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
});
