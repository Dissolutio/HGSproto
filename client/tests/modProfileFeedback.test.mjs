import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ModProfilePanel } from "../.test-build/game/components/ModProfilePanel.js";

const profile = {
  modId: "com.community.scout",
  displayName: "Community Scout",
  visualPrimitive: "CUBE",
  currentHealth: 5,
  maxHealth: 5,
};
const noop = () => {};
const props = {
  profileText: "",
  onProfileTextChange: noop,
  enabled: true,
  onLoadProfiles: noop,
  loadedProfiles: [profile],
  selectedProfile: profile,
  onSelectProfile: noop,
  deploymentSeat: "P1",
  onDeploymentSeatChange: noop,
  deploymentSlots: [],
  deploymentSlotId: "",
  onDeploymentSlotChange: noop,
  startingHealth: "5",
  onStartingHealthChange: noop,
  onDeployProfile: noop,
  setupLocked: false,
  matchFinished: false,
  onStartFresh: noop,
};

const renderPanel = (overrides = {}) =>
  renderToStaticMarkup(createElement(ModProfilePanel, { ...props, ...overrides }));
const region = (html, id) => {
  const match = html.match(new RegExp(`<p id="${id}"[^>]*>.*?</p>`, "s"));
  assert.ok(match, `Missing feedback region ${id}`);
  return match[0];
};
const assertAccessible = (markup) => {
  assert.match(markup, /role="status"/);
  assert.match(markup, /aria-live="polite"/);
  assert.match(markup, /aria-atomic="true"/);
  assert.match(markup, /overflow-wrap:anywhere/);
  assert.match(markup, /white-space:pre-wrap/);
};

test("empty live regions are present directly after the load and deploy buttons", () => {
  const html = renderPanel();
  for (const [label, id] of [
    ["Load Mod Profile", "mod-profile-load-feedback"],
    ["Deploy Profile into Roster Slot", "mod-profile-deploy-feedback"],
  ]) {
    assert.ok(html.includes(`${label}</button><p id="${id}"`));
    const markup = region(html, id);
    assertAccessible(markup);
    assert.match(markup, /><\/p>$/);
  }
});

test("load errors appear beside the text box even with no loaded profiles", () => {
  const html = renderPanel({
    loadedProfiles: [],
    selectedProfile: null,
    loadFeedback: { kind: "error", text: "unknown field `movement_speed`" },
  });
  const markup = region(html, "mod-profile-load-feedback");
  assertAccessible(markup);
  assert.match(markup, /Error: unknown field `movement_speed`/);
  assert.match(markup, /color:#ff9999/);
  assert.match(html, /aria-describedby="mod-profile-load-feedback"/);
  assert.doesNotMatch(html, /Loaded mod profiles/);
});

test("a successful load replaces the error with a count and profile names", () => {
  const html = renderPanel({
    loadFeedback: {
      kind: "success",
      text: "Loaded 2 community mod profiles: Community Scout, Community Guard",
    },
  });
  const markup = region(html, "mod-profile-load-feedback");
  assert.match(markup, /Loaded 2 community mod profiles: Community Scout, Community Guard/);
  assert.doesNotMatch(markup, /Error:|#ff9999/);
});

test("deploy reasons remain separate from load feedback, including locked setup", () => {
  for (const reason of [
    "Starting health must be a whole number from 1 to 5",
    "wrong slot: UNIT_P2_1",
    "setup locked",
  ]) {
    const html = renderPanel({
      setupLocked: true,
      loadFeedback: { kind: "success", text: "Loaded 1 community mod profile: Community Scout" },
      deployFeedback: { kind: "error", text: reason },
    });
    const markup = region(html, "mod-profile-deploy-feedback");
    assertAccessible(markup);
    assert.ok(markup.includes(`Error: ${reason}`));
    assert.match(markup, /color:#ff9999/);
    assert.match(html, /aria-describedby="mod-profile-deploy-feedback"/);
    assert.doesNotMatch(region(html, "mod-profile-load-feedback"), /Error:/);
  }
});

test("long multiline core errors are preserved as escaped text, not markup", () => {
  const reason = `unknown field ${"x".repeat(1000)}\n<script>alert("bad")</script>`;
  const html = renderPanel({ loadFeedback: { kind: "error", text: reason } });
  const markup = region(html, "mod-profile-load-feedback");
  assertAccessible(markup);
  assert.ok(markup.includes("x".repeat(1000)));
  assert.ok(markup.includes("\n&lt;script&gt;"));
  assert.doesNotMatch(markup, /<script>/);
});

test("the next attempt can clear either feedback region independently", () => {
  const error = { kind: "error", text: "core rejection" };
  const loadCleared = renderPanel({ loadFeedback: null, deployFeedback: error });
  assert.match(region(loadCleared, "mod-profile-load-feedback"), /><\/p>$/);
  assert.match(region(loadCleared, "mod-profile-deploy-feedback"), /Error: core rejection/);
  const deployCleared = renderPanel({ loadFeedback: error, deployFeedback: null });
  assert.match(region(deployCleared, "mod-profile-deploy-feedback"), /><\/p>$/);
  assert.match(region(deployCleared, "mod-profile-load-feedback"), /Error: core rejection/);
});
