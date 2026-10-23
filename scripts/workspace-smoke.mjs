// Browser plugin not available: use installed Chrome with Playwright.
// Run against an isolated studio-server on 8081; never modify the native app's document.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
const api = process.env.STUDIO_TEST_API || "http://127.0.0.1:8081";
assert.notEqual(new URL(api).port, "8080", "Use an isolated test server");
const origin = process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180";
const browser = await chromium.launch({
  executablePath:
    process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ||
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  headless: true,
});
const session = await fetch(api + "/studio/session", {
  method: "POST",
  headers: { Origin: origin },
}).then((r) => r.json());
const headers = {
  "Content-Type": "application/json",
  "x-studio-token": session.token,
};
async function state() {
  return fetch(api + "/studio/state", { headers }).then((r) => r.json());
}
async function action(action, rest = {}) {
  const r = await fetch(api + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...rest }),
  });
  assert.equal(r.status, 200, await r.text());
}
const baseline = await state();
const errors = [];
async function page(url = origin) {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", (e) => errors.push(e.message));
  p.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  await p.route(origin + "/studio/**", async (route) => {
    const response = await route.fetch({
      url: route.request().url().replace(origin, api),
    });
    const host = new URL(url).searchParams.get("panel");
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
  await p.goto(url);
  await p
    .getByRole("tablist", { name: /Workspace|ワークスペース|工作区/ })
    .waitFor();
  assert.equal(await p.title(), "Soliton");
  assert.equal(await p.locator("vite-error-overlay").count(), 0);
  return p;
}
async function order(p, expected) {
  await p.waitForFunction(
    (expected) =>
      Array.from(document.querySelectorAll('.workspace-tabs [role="tab"]'))
        .map((e) => e.id.replace("workspace-", ""))
        .join(",") === expected.join(","),
    expected,
  );
}
try {
  await action("resetWorkspace");
  await action("settings", {
    settings: { ...baseline.settings, language: "en" },
  });
  const p = await page(),
    other = await page();
  await p.getByRole("tab", { name: "Logs", exact: true }).click();
  await other.locator('#workspace-logs[aria-selected="true"]').waitFor();
  await p.getByRole("button", { name: "Move tab left", exact: true }).click();
  await order(p, ["logs", "chat"]);
  await order(other, ["logs", "chat"]);
  await p.reload();
  await order(p, ["logs", "chat"]);
  await p.locator("#workspace-logs").dragTo(p.locator("#workspace-chat"));
  await order(p, ["chat", "logs"]);
  await order(other, ["chat", "logs"]);
  await p.locator("#workspace-logs").press("Alt+ArrowLeft");
  await order(p, ["logs", "chat"]);
  await p.locator("#workspace-logs").press("ArrowRight");
  await p.locator('#workspace-chat[aria-selected="true"]').waitFor();
  await other.locator('#workspace-chat[aria-selected="true"]').waitFor();
  assert(await p.locator(".composer").isVisible());
  // Existing content is identical across views; movement never sends message mutations.
  assert.deepEqual(
    (await state()).document.messages,
    baseline.document.messages,
  );
  const invalid = await fetch(api + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({
      action: "moveWorkspaceTab",
      tab: "chat",
      before: "unknown",
    }),
  });
  assert.equal(invalid.status, 400);
  for (const [language, theme] of [
    ["ja", "dark"],
    ["en", "light"],
    ["zh", "dark"],
  ]) {
    await action("settings", {
      settings: { ...baseline.settings, language, theme },
    });
    await p.reload();
    for (const width of [1440, 390, 320]) {
      await p.setViewportSize({ width, height: 800 });
      await p.locator(".workspace-bar").waitFor();
      assert(
        await p.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      );
      const box = await p.locator(".workspace-bar").boundingBox();
      assert(box.x >= 0 && box.x + box.width <= width);
      assert(await p.locator(".workspace-tools button").last().isDisabled());
    }
  }
  await action("settings", {
    settings: { ...baseline.settings, language: "ja" },
  });
  await p.setViewportSize({ width: 1100, height: 760 });
  await p.reload();
  await p.screenshot({ path: "/tmp/strata-workspace-tabs.png" });
  // Native-only branch rendered with a browser query: same authoritative layout/content.
  const floating = await page(origin + "?panel=workspace");
  await floating.locator(".workspace-bar").waitFor();
  assert(await floating.locator(".composer").isVisible());
  assert.equal(
    await floating
      .locator(".workspace .panel-header .tools button[title*='パネルを戻す']")
      .count(),
    0,
  );
  assert.deepEqual(errors, []);
  console.log(
    "PASS: two-view selection/order sharing; button, drag and Alt+arrow reorder; keyboard navigation; reload; invalid request; message preservation; EN/JA/ZH light/dark at 1440/390/320; floating-group content; no overlay/console errors. Native window lifecycle requires native QA.",
  );
} finally {
  await action("settings", { settings: baseline.settings });
  await action("resetWorkspace");
  if (baseline.document.layout.workspaceOrder[0] === "logs")
    await action("moveWorkspaceTab", { tab: "logs", before: "chat" });
  await action("workspaceTab", {
    tab: baseline.document.layout.workspaceActive,
  });
  await browser.close();
}
