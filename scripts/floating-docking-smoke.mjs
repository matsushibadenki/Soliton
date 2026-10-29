// Browser plugin not available. Native IPC is emulated; routing uses the real Rust fixture.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
const origin = process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180",
  api = process.env.STUDIO_TEST_API || "http://127.0.0.1:8081";
const session = await fetch(api + "/studio/session", {
  method: "POST",
  headers: { Origin: origin },
}).then((r) => r.json());
const headers = {
  "Content-Type": "application/json",
  "x-studio-token": session.token,
};
const state = () =>
  fetch(api + "/studio/state", { headers }).then((r) => r.json());
async function command(value) {
  const r = await fetch(api + "/fixture/command", {
    method: "POST",
    headers,
    body: JSON.stringify(value),
  });
  const v = await r.json();
  assert.equal(r.status, 200, JSON.stringify(v));
  return v;
}
async function action(action, data = {}) {
  const r = await fetch(api + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...data }),
  });
  assert.equal(r.status, 200, await r.text());
}
const baseline = await state();
const browser = await chromium.launch({
  executablePath:
    process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ||
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  headless: true,
});
const errors = [],
  retired = [];
async function open(host = "main") {
  const p = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  p.on("pageerror", (e) => errors.push(e.message));
  p.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  await p.exposeFunction("fixtureInvoke", async (name, args) => {
    if (name === "connection") return { url: origin, token: session.token };
    if (name === "dock_workspace_tab") {
      const result = await command({ command: "dock", host, ...args });
      retired.push(...result.retired);
      return null;
    }
    if (name === "float_panel")
      return command({ command: "float", group: args.panel });
    if (name === "dock_panel")
      return command({ command: "return", group: args.panel });
    return null;
  });
  await p.addInitScript(
    ({ host }) => {
      let id = 0;
      window.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: host === "main" ? "main" : "panel-" + host },
          currentWebview: { label: host },
        },
        transformCallback: () => ++id,
        unregisterCallback: () => {},
        invoke: (name, args) => window.fixtureInvoke(name, args),
      };
    },
    { host },
  );
  await p.route(origin + "/studio/**", (route) =>
    route
      .fetch({ url: route.request().url().replace(origin, api) })
      .then((response) => route.fulfill({ response })),
  );
  await p.goto(origin + (host === "main" ? "" : "?panel=" + host));
  await p.waitForFunction(() => document.title === "Soliton");
  assert.equal(await p.title(), "Soliton");
  return p;
}
async function drop(
  source,
  target,
  tab,
  group,
  placement,
  { plain = false } = {},
) {
  // Capture the source's real dragstart payload, then transport it across distinct WebViews.
  const payload = await source
    .locator("#workspace-" + tab)
    .evaluate((element) => {
      const data = new DataTransfer();
      element.dispatchEvent(
        new DragEvent("dragstart", { bubbles: true, dataTransfer: data }),
      );
      return Object.fromEntries(
        data.types.map((type) => [type, data.getData(type)]),
      );
    });
  assert.equal(payload["text/plain"], "strata-workspace-tab:" + tab);
  await target.locator(`[data-group="${group}"]`).evaluate(
    (element, payload) => {
      const data = new DataTransfer();
      for (const [type, value] of Object.entries(payload))
        data.setData(type, value);
      element.dispatchEvent(
        new DragEvent("dragenter", {
          bubbles: Object.keys(payload).length > 1,
          dataTransfer: data,
        }),
      );
    },
    plain ? { "text/plain": payload["text/plain"] } : payload,
  );
  // WebKit can report a null relatedTarget when a child is replaced by the overlay.
  const slot = target.locator(`[data-group="${group}"]`);
  await slot.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    element.dispatchEvent(
      new DragEvent("dragleave", {
        bubbles: false,
        relatedTarget: null,
        clientX: rect.left + rect.width / 2,
        clientY: rect.top + rect.height / 2,
      }),
    );
  });
  await target.locator(`[data-group="${group}"] .dock-targets`).waitFor();
  // Leaving the slot clears it; dragover must restore it without a second dragenter.
  await slot.evaluate((element) =>
    element.dispatchEvent(
      new DragEvent("dragleave", {
        bubbles: false,
        relatedTarget: null,
        clientX: -10,
        clientY: -10,
      }),
    ),
  );
  await target
    .locator(`[data-group="${group}"] .dock-targets`)
    .waitFor({ state: "hidden" });
  await slot.evaluate(
    (element, payload) => {
      const data = new DataTransfer();
      for (const [type, value] of Object.entries(payload))
        data.setData(type, value);
      const rect = element.getBoundingClientRect();
      element.dispatchEvent(
        new DragEvent("dragover", {
          bubbles: Object.keys(payload).length > 1,
          cancelable: true,
          dataTransfer: data,
          clientX: rect.left + rect.width / 2,
          clientY: rect.top + rect.height / 2,
        }),
      );
    },
    plain ? { "text/plain": payload["text/plain"] } : payload,
  );
  const zone = target.locator(`[data-group="${group}"] .dock-${placement}`);
  await zone.waitFor();
  await zone.evaluate(
    (element, payload) => {
      const data = new DataTransfer();
      for (const [type, value] of Object.entries(payload))
        data.setData(type, value);
      element.dispatchEvent(
        new DragEvent("drop", {
          bubbles: Object.keys(payload).length > 1,
          cancelable: true,
          dataTransfer: data,
        }),
      );
    },
    plain ? { "text/plain": payload["text/plain"] } : payload,
  );
}
try {
  await action("resetWorkspace");
  await action("settings", {
    settings: { ...baseline.settings, language: "en" },
  });
  const main = await open();
  await main.locator(".workspace-bar").waitFor();
  await command({ command: "float", group: "workspace" });
  const whole = await open("workspace");
  await whole.locator("#workspace-logs").waitFor();
  await main.locator(".workspace-detached").waitFor();
  // Joined floating group -> main edge, including the main placeholder as destination.
  await drop(whole, main, "logs", "workspace", "right", { plain: true });
  await main
    .locator('.split-columns [data-group="group-logs"] .dock-group')
    .waitFor();
  assert(retired.includes("workspace"));
  assert.deepEqual((await state()).document.workspaceHosts, {});
  await whole.close();
  await command({ command: "float", group: "group-chat" });
  await command({ command: "float", group: "group-logs" });
  const chat = await open("group-chat"),
    logs = await open("group-logs");
  await chat.locator("#workspace-chat").waitFor();
  await logs.locator("#workspace-logs").waitFor();
  // Floating -> floating merge preserves destination host alias despite group identity changing.
  await drop(chat, logs, "chat", "group-logs", "center");
  await logs.locator('[data-group="workspace"] #workspace-chat').waitFor();
  assert.deepEqual((await state()).document.workspaceHosts, {
    workspace: "group-logs",
  });
  assert(retired.includes("group-chat"));
  await chat.close();
  // Split within a retained floating host: both groups render in that same window.
  await drop(logs, logs, "chat", "workspace", "top");
  await logs.locator('.split-rows [data-group="group-chat"]').waitFor();
  await logs.screenshot({ path: "/tmp/strata-floating-direct-split.png" });
  assert.deepEqual((await state()).document.workspaceHosts, {
    "group-chat": "group-logs",
    "group-logs": "group-logs",
  });
  // Return one group without closing the host containing its sibling.
  const one = await command({ command: "return", group: "group-chat" });
  assert.deepEqual(one.retired, []);
  await logs.locator('[data-group="group-chat"]').waitFor({ state: "hidden" });
  await main.locator('[data-group="group-chat"] .dock-group').waitFor();
  // Remaining floating group -> main center, no intermediate return action.
  await drop(logs, main, "logs", "group-chat", "center", { plain: true });
  await main.locator('[data-group="workspace"] #workspace-logs').waitFor();
  assert.deepEqual((await state()).document.workspaceHosts, {});
  assert(retired.filter((id) => id === "group-logs").length >= 1);
  // Invalid plain-text drags never mutate layout.
  const previous = (await state()).document.layout;
  await main.locator('[data-group="workspace"]').evaluate((element) => {
    const data = new DataTransfer();
    data.setData("text/plain", "chat");
    element.dispatchEvent(
      new DragEvent("dragenter", {
        bubbles: false,
        dataTransfer: data,
      }),
    );
  });
  await main.locator(".dock-center").evaluate((element) => {
    const data = new DataTransfer();
    data.setData("text/plain", "chat");
    element.dispatchEvent(
      new DragEvent("drop", {
        bubbles: true,
        cancelable: true,
        dataTransfer: data,
      }),
    );
  });
  assert.deepEqual((await state()).document.layout, previous);
  assert.deepEqual(
    (await state()).document.messages,
    baseline.document.messages,
  );
  assert.equal(await main.locator("vite-error-overlay").count(), 0);
  assert.deepEqual(errors, []);
  await main.screenshot({ path: "/tmp/strata-floating-docking.png" });
  console.log(
    "PASS: real Rust host routing + cross-WebView drag payload transport, plain-text Cocoa fallback, floating-to-main split via placeholder, floating-to-floating merge, split inside retained floating host, partial return, direct merge to main, retired-host bookkeeping, invalid-payload rejection, message preservation, no overlay/console errors. Native OS pointer drag is not covered by this fixture.",
  );
} finally {
  const current = await state();
  for (const group of Object.keys(current.document.workspaceHosts))
    await command({ command: "return", group });
  await action("resetWorkspace");
  await action("settings", { settings: baseline.settings });
  await browser.close();
}
