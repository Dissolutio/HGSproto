import assert from "node:assert/strict";
import { after, test } from "node:test";
import { createServer } from "vite";

const server = await createServer({
  configFile: false,
  server: { middlewareMode: true },
});
after(() => server.close());
const { GameHud } = await server.ssrLoadModule("/src/game/components/GameHud.tsx");

function elements(node) {
  if (!node || typeof node !== "object") return [];
  return [node, ...[node.props?.children].flat().flatMap(elements)];
}

test("refused restore notice keeps its message and controls beside ordinary notices", () => {
  const onStartFreshSandbox = () => {};
  const onDismissSandboxRestore = () => {};
  const props = {
    sandboxRestoreNotice: "Saved copy kept aside",
    onStartFreshSandbox,
    onDismissSandboxRestore,
  };
  for (const notice of ["Sandbox ready", "Action submitted"]) {
    const nodes = elements(GameHud({ ...props, notice }));
    const banner = nodes.find((node) => node.type === "aside" && node.props.role === "alert");
    assert.ok(banner);
    const children = elements(banner);
    assert.ok(children.some((node) => node.props?.children === props.sandboxRestoreNotice));
    const startFresh = children.find((node) => node.props?.children === "Start Fresh Sandbox");
    assert.equal(startFresh.type, "button");
    assert.equal(startFresh.props.onClick, onStartFreshSandbox);
    const dismiss = children.find(
      (node) => node.props?.["aria-label"] === "Dismiss sandbox restore notification",
    );
    assert.equal(dismiss.props.onClick, onDismissSandboxRestore);
  }
});

test("no restore banner or extra Start Fresh button after dismissal or without a refusal", () => {
  for (const sandboxRestoreNotice of [null, undefined]) {
    const nodes = elements(GameHud({ sandboxRestoreNotice }));
    assert.equal(nodes.some((node) => node.type === "aside"), false);
    assert.equal(nodes.some((node) => node.props?.children === "Start Fresh Sandbox"), false);
  }
});
