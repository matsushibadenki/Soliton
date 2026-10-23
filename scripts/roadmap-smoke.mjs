import { preferences, closePreferences, language } from "./ui-helpers.mjs";
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, symlink, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
});
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});
const fixture = await mkdtemp(join(tmpdir(), "strata-config-ui-"));
let baseline;
let headers;
async function state() {
  return fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/studio/state",
    { headers },
  ).then((r) => r.json());
}
async function action(action, data = {}) {
  const r = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
      "/studio/action",
    {
      method: "POST",
      headers,
      body: JSON.stringify({ action, ...data }),
    },
  );
  assert.equal(r.status, 200, await r.text());
}
try {
  await page.goto(process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180");
  await language(page, "English");
  await closePreferences(page);
  const connection = await page.evaluate(async () =>
    fetch("/studio/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    }).then((r) => r.json()),
  );
  headers = {
    "Content-Type": "application/json",
    "x-studio-token": connection.token,
  };
  baseline = (await state()).settings;
  await mkdir(join(fixture, "metal"));
  await mkdir(join(fixture, "tokenizer"));
  await symlink(resolve("Strata/serve"), join(fixture, "serve"));
  await writeFile(
    join(fixture, "metal/setup_mac.py"),
    "raise SystemExit('UI fixture: never downloads models')\n",
  );
  await writeFile(join(fixture, "engine"), "fixture", { mode: 0o755 });
  await writeFile(join(fixture, "tokenizer/vocab.json"), "{}");
  const configPath = join(fixture, "strata-ready.json");
  await writeFile(
    configPath,
    JSON.stringify({
      exe: join(fixture, "engine"),
      args: [],
      tokenizer: join(fixture, "tokenizer"),
      backend: "metal",
      model_name: "Local fixture",
    }),
  );
  await writeFile(join(fixture, "strata-invalid.json"), "{broken");
  await action("settings", {
    settings: {
      ...baseline,
      strataRoot: fixture,
      python: process.env.STUDIO_TEST_PYTHON || "python3",
      language: "en",
    },
  });
  await page.reload();
  await preferences(page, "engine");
  await page
    .getByRole("button", { name: "Prepare model", exact: true })
    .click();
  const dialog = page.locator(".setup-dialog");
  await dialog.waitFor();
  await page
    .getByRole("combobox", { name: "Installed configurations" })
    .selectOption(configPath);
  await page.waitForFunction(
    () =>
      document.querySelector(".diagnostic:nth-child(4) code")?.textContent ===
      "Available",
  );
  const fresh = await state();
  assert.equal(fresh.settings.config, configPath);
  const inspection = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
      "/studio/inspect",
    {
      headers,
    },
  ).then((r) => r.json());
  assert(
    inspection.selected.engineExists && inspection.selected.tokenizerExists,
  );
  assert.equal(inspection.configs.length, 2);
  assert(
    inspection.configs.find((c) => c.path.endsWith("strata-invalid.json"))
      .error,
  );
  assert.equal(inspection.models[0].downloadGb, 66.4);
  assert(
    await dialog
      .getByRole("button", { name: "Prepare model", exact: true })
      .isDisabled(),
  );
  assert.equal((await state()).preparation.status, "idle");
  const noConsent = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
      "/studio/action",
    {
      method: "POST",
      headers,
      body: JSON.stringify({
        action: "prepare",
        model: "Q2_0",
        confirmed: false,
      }),
    },
  );
  assert.equal(noConsent.status, 400);
  assert.equal((await state()).preparation.status, "idle");
  await dialog
    .getByRole("combobox", { name: "Model", exact: true })
    .selectOption("IQ3_S");
  assert(await dialog.getByText("83.6 GB", { exact: true }).isVisible());
  await page.screenshot({ path: "/tmp/strata-roadmap-setup.png" });
  for (const width of [320, 375, 768]) {
    await page.setViewportSize({ width, height: 960 });
    assert(
      await dialog.evaluate(
        (e) =>
          e.getBoundingClientRect().left >= 0 &&
          e.getBoundingClientRect().right <= innerWidth,
      ),
    );
    assert(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    );
  }
  await dialog.getByRole("button", { name: "Close", exact: true }).click();
  await page.setViewportSize({ width: 1440, height: 960 });
  await preferences(page, "api");
  assert(await page.locator(".preferences-dialog .api").isVisible());
  await closePreferences(page);
  await page.getByRole("tab", { name: "Logs", exact: true }).click();
  assert(await page.locator(".log-lines").isVisible());
  assert(!(await page.locator(".composer").isVisible()));
  await page
    .getByRole("tab", { name: "Chat", exact: true })
    .press("ArrowRight");
  await page.getByRole("tab", { name: "Logs", exact: true }).press("ArrowLeft");
  assert(await page.locator(".composer").isVisible());
  for (const [label, title] of [
    ["日本語", "モデルを準備"],
    ["简体中文", "准备模型"],
  ]) {
    await language(page, label);
    await preferences(page, "engine");
    await page.getByRole("button", { name: title, exact: true }).click();
    await page
      .locator(".setup-dialog")
      .getByRole("heading", { name: title, exact: true })
      .waitFor();
    await page.keyboard.press("Escape");
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: setup diagnostics, config discovery/selection, no automatic download, consent enforcement, three dialog languages, dialog responsiveness, tabbed preferences and workspace, keyboard tab navigation.",
  );
} finally {
  if (baseline && headers) {
    await action("settings", { settings: baseline });
    await action("layout", {
      layout: { engineWidth: 270, apiWidth: 300, logsHeight: 174 },
    });
  }
  await browser.close();
  await rm(fixture, { recursive: true, force: true });
}
