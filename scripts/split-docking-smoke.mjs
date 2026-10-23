// Browser plugin not available: Playwright + installed Chrome; isolated API only.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
const origin = process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180",
  api = process.env.STUDIO_TEST_API || "http://127.0.0.1:8081";
assert.notEqual(new URL(api).port, "8080", "Use an isolated server");
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
const errors = [];
async function open(query = "") {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", (e) => errors.push(e.message));
  p.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  await p.route(origin + "/studio/**", async (route) => {
    const response = await route.fetch({
      url: route.request().url().replace(origin, api),
    });
    const host = new URL(origin + query).searchParams.get("panel");
    if (host && route.request().url().endsWith("/studio/state")) {
      const value = await response.json();
      value.document.workspaceHosts = Object.fromEntries(
        value.document.workspaceGroups
          .filter((group) => group.id === host)
          .map((group) => [group.id, host]),
      );
      await route.fulfill({ response, json: value });
    } else await route.fulfill({ response });
  });
  await p.goto(origin + query);
  await p.locator(".workspace-bar").first().waitFor();
  assert.equal(await p.title(), "Soliton");
  assert.equal(await p.locator("vite-error-overlay").count(), 0);
  return p;
}
async function split(p, axis) {
  await p.waitForFunction(
    (axis) =>
      !!document.querySelector(
        axis === "horizontal"
          ? ".workspace-dock.split-columns"
          : ".workspace-dock.split-rows",
      ),
    axis,
  );
  assert.equal(await p.locator(".dock-group").count(), 2);
}
async function menu(p, group, label) {
  await p
    .locator(`[data-group="${group}"]`)
    .getByRole("button", { name: "Dock layout", exact: true })
    .click();
  await p.getByRole("menuitem", { name: label, exact: true }).click();
}
async function drag(p, source, targetGroup, position) {
  const a = await p.locator(`#workspace-${source}`).boundingBox();
  const group = p.locator(`[data-group="${targetGroup}"]`),
    b = await group.boundingBox();
  await p.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await p.mouse.down();
  await p.mouse.move(a.x + a.width / 2 + 12, a.y + a.height / 2, { steps: 4 });
  await p.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 12 });
  const drop = group.locator(".dock-" + position);
  await drop.waitFor();
  const r = await drop.boundingBox();
  await p.mouse.move(r.x + r.width / 2, r.y + r.height / 2, { steps: 10 });
  await p.mouse.up();
}
try {
  await action("resetWorkspace");
  await action("settings", {
    settings: { ...baseline.settings, language: "en" },
  });
  const messages = [
    { role: "user", content: "Keep this conversation while docking." },
    { role: "assistant", content: "The conversation stays in Rust." },
  ];
  await action("messages", { messages });
  const p = await open(),
    replica = await open();
  await p.locator(".composer textarea").fill("unsent draft");
  await menu(p, "workspace", "Dock right");
  await split(p, "horizontal");
  await split(replica, "horizontal");
  assert.equal(
    (await state()).document.layout.workspaceOrder.join(),
    "logs,chat",
  );
  assert.equal(
    await p.locator(".composer textarea").inputValue(),
    "unsent draft",
  );
  assert(await p.locator(".log-lines").isVisible());
  assert(await p.locator(".conversation").isVisible());
  const separator = p.getByRole("separator", {
    name: "Resize workspace groups",
  });
  await separator.press("ArrowRight");
  await p.waitForFunction(
    () =>
      document
        .querySelector('[role="separator"]')
        .getAttribute("aria-valuenow") === "55",
  );
  const bounds = await p.locator(".workspace-dock").boundingBox(),
    handle = await separator.boundingBox();
  await p.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
  await p.mouse.down();
  await p.mouse.move(
    bounds.x + bounds.width * 0.65,
    handle.y + handle.height / 2,
    { steps: 8 },
  );
  await p.mouse.up();
  await p.waitForFunction(
    () =>
      document
        .querySelector('[role="separator"]')
        .getAttribute("aria-valuenow") === "65",
  );
  assert.equal((await state()).document.layout.workspaceRatio, 65);
  await p.reload();
  await split(p, "horizontal");
  assert.equal(await separator.getAttribute("aria-valuenow"), "65");
  await menu(p, "group-logs", "Dock below");
  await split(p, "vertical");
  assert.equal(
    (await state()).document.layout.workspaceOrder.join(),
    "chat,logs",
  );
  await menu(p, "group-logs", "Merge into tab group");
  await p.locator('[data-group="workspace"]').waitFor();
  assert.equal(await p.locator(".dock-group").count(), 1);
  // Native HTML drag payload, split via edge target then merge via center target.
  await drag(p, "logs", "workspace", "right");
  await split(p, "horizontal");
  await drag(p, "logs", "group-chat", "center");
  await p.locator('[data-group="workspace"]').waitFor();
  await menu(p, "workspace", "Dock left");
  await split(p, "horizontal");
  const isolated = await open("?panel=group-logs");
  assert.equal(await isolated.locator(".dock-group").count(), 1);
  assert(await isolated.locator(".log-lines").isVisible());
  assert.equal(await isolated.locator(".composer").count(), 0);
  await isolated.close();
  for (const [language, theme] of [
    ["en", "light"],
    ["ja", "dark"],
    ["zh", "light"],
  ]) {
    await action("settings", {
      settings: { ...baseline.settings, language, theme },
    });
    await p.reload();
    for (const width of [1440, 390, 320]) {
      await p.setViewportSize({ width, height: 800 });
      await p.waitForFunction(
        (width) =>
          document
            .querySelector('[role="separator"]')
            ?.getAttribute("aria-orientation") ===
          (width < 680 ? "horizontal" : "vertical"),
        width,
      );
      assert(
        await p.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      );
      assert.equal(await p.locator(".dock-group").count(), 2);
      for (const bar of await p.locator(".workspace-bar").all()) {
        const box = await bar.boundingBox();
        assert(box.x >= 0 && box.x + box.width <= width);
      }
      const composer = await p.locator(".composer").boundingBox();
      const content = await p
        .locator('[data-group="group-chat"] .workspace-content')
        .boundingBox();
      assert(
        composer.y + composer.height <= content.y + content.height + 1,
        "Composer must fit above tab controls",
      );
      // Portal menu must fit even in a narrow split slot.
      await p.locator(".dock-menu button").first().click();
      const box = await p.locator('[role="menu"]').boundingBox();
      assert(
        box.x >= 0 &&
          box.y >= 0 &&
          box.x + box.width <= width &&
          box.y + box.height <= 800,
      );
      await p.keyboard.press("Escape");
      assert.equal(await p.locator('[role="menu"]').count(), 0);
    }
  }
  await action("settings", {
    settings: { ...baseline.settings, language: "ja", theme: "dark" },
  });
  await p.setViewportSize({ width: 1440, height: 900 });
  await p.reload();
  await p.screenshot({ path: "/tmp/strata-split-docking-desktop.png" });
  await p.setViewportSize({ width: 390, height: 844 });
  await split(p, "vertical");
  await p.screenshot({ path: "/tmp/strata-split-docking-narrow.png" });
  assert.deepEqual((await state()).document.messages, messages);
  assert.deepEqual(errors, []);
  console.log(
    "PASS: horizontal/vertical split, merge, real edge/center drag, shared two-view geometry, pointer/keyboard resize, reload, isolated group projection, draft/message preservation, EN/JA/ZH light/dark at 1440/390/320, menu bounds, no blank/overlay/console errors.",
  );
} finally {
  await action("messages", { messages: baseline.document.messages });
  await action("settings", { settings: baseline.settings });
  await action("resetWorkspace");
  if (baseline.document.layout.workspaceSplit)
    await action("dockWorkspaceTab", {
      tab: baseline.document.layout.workspaceOrder[0],
      target: baseline.document.layout.workspaceOrder[1],
      placement:
        baseline.document.layout.workspaceSplit === "horizontal"
          ? "left"
          : "top",
    });
  else if (baseline.document.layout.workspaceOrder[0] === "logs")
    await action("moveWorkspaceTab", { tab: "logs", before: "chat" });
  await action("workspaceTab", {
    tab: baseline.document.layout.workspaceActive,
  });
  await action("resizeWorkspace", {
    ratio: baseline.document.layout.workspaceRatio,
  });
  await browser.close();
}
