import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, copyFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "@playwright/test";
import { preferences, closePreferences, language } from "./ui-helpers.mjs";

// Browser plugin skill is not available; use the repository's Playwright workflow.
const fixture = await mkdtemp(join(tmpdir(), "soliton-engines-"));
const base = process.env.SOLITON_TEST_URL || "http://127.0.0.1:18080";
const python =
  process.env.STUDIO_TEST_PYTHON || resolve("Strata/.venv/bin/python");
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
});
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
let server;
let headers;
let output = "";
async function waitFor(fn, label) {
  for (let i = 0; i < 150; i++) {
    if (await fn()) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error("Timed out: " + label + "\n" + output.slice(-2000));
}
async function startServer() {
  server = spawn(resolve("target/debug/studio-server"), {
    env: {
      ...process.env,
      STUDIO_PORT: new URL(base).port,
      STUDIO_STATE_PATH: join(fixture, "state.json"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  server.stdout.on("data", (b) => (output += b));
  server.stderr.on("data", (b) => (output += b));
  await waitFor(
    async () =>
      fetch(base + "/health")
        .then((r) => r.ok)
        .catch(() => false),
    "gateway start",
  );
  const c = await fetch(base + "/studio/session", {
    method: "POST",
    headers: { "Content-Type": "application/json", Origin: base },
    body: "{}",
  }).then((r) => r.json());
  headers = { "Content-Type": "application/json", "x-studio-token": c.token };
}
async function state() {
  return fetch(base + "/studio/state", { headers }).then((r) => r.json());
}
async function act(action, data = {}, ok = true) {
  const r = await fetch(base + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...data }),
  });
  assert.equal(r.ok, ok, await r.clone().text());
  return r.json();
}
async function stopServer() {
  if (!server || server.exitCode !== null) return;
  const done = new Promise((r) => server.once("exit", r));
  server.kill("SIGINT");
  await done;
}
try {
  await mkdir(join(fixture, "vllm_apple"));
  await mkdir(join(fixture, "model"));
  await copyFile(
    "scripts/fixtures/vllm_cli.py",
    join(fixture, "vllm_apple/cli.py"),
  );
  await writeFile(
    join(fixture, "vllm_apple/__main__.py"),
    "from .cli import main\nimport sys\nsys.exit(main(sys.argv[1:]))\n",
  );
  await startServer();
  let settings = (await state()).settings;
  await act("settings", {
    settings: {
      ...settings,
      language: "en",
      mock: true,
      python,
      vllm: {
        root: fixture,
        python,
        model: join(fixture, "model"),
        backend: "mlx_lm",
        executable: python,
      },
    },
  });
  await page.goto(base);
  assert.equal(await page.title(), "Soliton");
  await preferences(page, "engine");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await waitFor(
    async () => (await state()).status === "ready",
    "Strata mock ready",
  );
  await closePreferences(page);
  await page
    .getByRole("textbox", { name: "Type a message…" })
    .fill("Keep this history");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await waitFor(async () => {
    const s = await state();
    return s.document.messages.length === 2 && !s.document.generating;
  }, "Strata chat");
  assert.equal(
    (await state()).document.messages[1].inference.metrics.engine,
    "Strata",
  );
  await preferences(page, "engine");
  await page
    .getByRole("combobox", { name: "Inference engine", exact: true })
    .selectOption("vllmApple");
  await waitFor(async () => {
    const s = await state();
    return s.settings.engine === "vllmApple" && s.status === "offline";
  }, "stop then select Apple");
  assert.equal((await state()).document.messages.length, 2);
  await page.getByRole("textbox", { name: "Local model directory" }).waitFor();
  assert.equal(
    await page.getByRole("textbox", { name: "Strata directory" }).count(),
    0,
  );
  await page
    .getByRole("button", { name: "Inspect model and evidence", exact: true })
    .click();
  await page.locator('[data-inspection-status="unverified"]').waitFor();
  assert.equal(
    (await state()).vllmInspection.architecture,
    "Gemma2ForCausalLM",
  );
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await waitFor(async () => {
    const s = await state();
    return s.status === "starting" && s.health?.control_ready;
  }, "control-ready is not inference-ready");
  await waitFor(async () => (await state()).status === "ready", "Apple ready");
  await act("inspectVllm", {}, false);
  assert.equal(
    (await fetch(base + "/v1/models", { headers }).then((r) => r.json()))
      .data[0].id,
    "default_model",
  );
  await page.screenshot({ path: "/tmp/soliton-engine-desktop.png" });
  await closePreferences(page);
  await page
    .getByRole("textbox", { name: "Type a message…" })
    .fill("Continue with Apple");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await waitFor(
    async () => (await state()).document.generating,
    "Apple generating",
  );
  await act("switchEngine", { engine: "strata" }, false);
  await waitFor(
    async () => !(await state()).document.generating,
    "Apple finish",
  );
  let s = await state();
  assert.equal(s.document.messages.length, 4);
  assert.equal(s.document.messages[3].content, "Apple fixture response");
  assert.equal(s.document.messages[3].inference.metrics.engine, "vLLM-Apple");
  assert.equal(s.document.messages[3].inference.metrics.usage.completion, 3);
  assert(
    s.requests.some((m) => m.engine === "Strata") &&
      s.requests.some((m) => m.engine === "vLLM-Apple"),
  );
  const api = await fetch(base + "/v1/chat/completions", {
    method: "POST",
    headers,
    body: JSON.stringify({
      model: "default_model",
      messages: [{ role: "user", content: "API" }],
      stream: true,
    }),
  });
  const reader = api.body.getReader();
  await reader.read();
  await act("switchEngine", { engine: "strata" }, false);
  while (!(await reader.read()).done) {}
  await waitFor(async () => {
    const r = await fetch(base + "/studio/action", {
      method: "POST",
      headers,
      body: JSON.stringify({ action: "switchEngine", engine: "strata" }),
    });
    return r.ok;
  }, "stream released");
  s = await state();
  assert.equal(s.status, "offline");
  assert.equal(s.document.messages.length, 4);
  const worker = s.logs
    .map((l) => l.match(/\[fixture\] worker (\d+)/)?.[1])
    .find(Boolean);
  assert(worker);
  assert.throws(() => process.kill(Number(worker), 0));
  await act("switchEngine", { engine: "vllmApple" });
  await page.reload();
  for (const [label, selector] of [
    ["日本語", "推論エンジン"],
    ["简体中文", "推理引擎"],
    ["English", "Inference engine"],
  ]) {
    await language(page, label);
    await preferences(page, "engine");
    await page.getByRole("combobox", { name: selector, exact: true }).waitFor();
    await page.setViewportSize({ width: 320, height: 960 });
    await page.locator('[data-inspection-status="unverified"]').waitFor();
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      true,
    );
    await page.screenshot({ path: `/tmp/soliton-inspection-${label}.png` });
  }
  for (const width of [320, 390, 1440]) {
    await page.setViewportSize({ width, height: 960 });
    await page.locator('[data-inspection-status="unverified"]').waitFor();
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      true,
    );
    await page.screenshot({ path: `/tmp/soliton-inspection-${width}.png` });
  }
  const beforeEdit = (await state()).settings;
  await act("settings", {
    settings: { ...beforeEdit, vllm: { ...beforeEdit.vllm, context: 2048 } },
  });
  assert.equal((await state()).vllmInspection, null);
  await act("settings", { settings: beforeEdit });
  assert.equal((await state()).vllmInspection, null);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: "/tmp/soliton-engine-mobile.png" });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
    true,
  );
  await closePreferences(page);
  await page.setViewportSize({ width: 1440, height: 960 });
  await page.screenshot({ path: "/tmp/soliton-shared-chat.png" });
  await stopServer();
  await startServer();
  s = await state();
  assert.equal(s.settings.engine, "vllmApple");
  assert.equal(s.status, "offline");
  assert.equal(s.document.messages.length, 4);
  assert.equal(s.document.messages[3].inference.metrics.engine, "vLLM-Apple");
  assert.equal(errors.length, 0, errors.join("\n"));
  console.log(
    "PASS: Soliton branding, Strata→Apple→Strata stop/switch, readiness, shared chat/history/logs/metrics, private auth, shared metadata diagnostics and cache invalidation, active API stream lock, worker cleanup, EN/JA/ZH, 390/1440px, restart persistence",
  );
} finally {
  await stopServer();
  await browser.close();
  await rm(fixture, { recursive: true, force: true });
}
