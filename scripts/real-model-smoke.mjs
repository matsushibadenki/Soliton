// Explicit real-GPU integration test. Requires fresh vLLM-Apple qualification evidence.
// No downloads, matrix changes or --skip-* flags. Uses isolated Soliton state.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, readFile, writeFile, chmod, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";
import { chromium, expect } from "@playwright/test";
import { preferences, closePreferences, language } from "./ui-helpers.mjs";

const evidence = process.env.SOLITON_REAL_EVIDENCE;
assert(
  evidence,
  "Set SOLITON_REAL_EVIDENCE to a fresh, complete qualification report",
);
const root = resolve("vLLM-apple");
const model =
  process.env.SOLITON_REAL_MODEL || join(root, "models/gemma-2-2b-it-4bit");
const python =
  process.env.SOLITON_REAL_PYTHON || join(root, ".venv/bin/python");
const executable =
  process.env.SOLITON_REAL_EXECUTABLE ||
  "/opt/homebrew/opt/vllm-metal/libexec/bin/mlx_lm.server";
const native = process.env.SOLITON_REAL_NATIVE === "1";
const port = process.env.SOLITON_REAL_PORT || (native ? "8080" : "18082");
assert(!native || port === "8080", "Native runtime uses its fixed port 8080");
const base = `http://127.0.0.1:${port}`;
const dir = await mkdtemp(join(tmpdir(), "soliton-real-"));
const report = {
  schemaVersion: 1,
  runtime: native ? "Tauri native bundle" : "headless shared Rust core",
  startedAt: new Date().toISOString(),
  model,
  backend: "mlx_lm",
  context: 1024,
  concurrency: 1,
  compatibilityChecksSkipped: false,
  checks: {},
  requests: [],
};
const evidenceBytes = await readFile(evidence);
report.evidenceSha256 = createHash("sha256")
  .update(evidenceBytes)
  .digest("hex");
const binding = JSON.parse(evidenceBytes).architecture_evidence;
assert.equal(binding.scope, "text_smoke_30min");
assert.equal(binding.context_tokens, 1024);
let server, browser, headers;
let output = "";
const consoleErrors = [];
async function waitFor(fn, label, seconds = 180) {
  const end = Date.now() + seconds * 1000;
  while (Date.now() < end) {
    const value = await fn();
    if (value) return value;
    await new Promise((r) => setTimeout(r, 400));
  }
  const logs = headers ? (await state()).logs.slice(-12).join("\n") : "";
  throw new Error(`Timed out: ${label}\n${logs}\n${output.slice(-1500)}`);
}
async function state() {
  return fetch(base + "/studio/state", { headers }).then((r) => r.json());
}
async function act(action, data = {}) {
  const r = await fetch(base + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...data }),
  });
  assert(r.ok, await r.clone().text());
  return r.json();
}
async function startServer() {
  const binary = native
    ? resolve(
        "target/aarch64-apple-darwin/release/bundle/macos/Soliton.app/Contents/MacOS/soliton",
      )
    : resolve("target/debug/studio-server");
  assert.equal(
    await fetch(base + "/health")
      .then(() => true)
      .catch(() => false),
    false,
    "Test port must be free",
  );
  server = spawn(binary, {
    env: {
      ...process.env,
      STUDIO_PORT: port,
      STUDIO_STATE_PATH: join(dir, "state.json"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  server.stdout.on("data", (b) => (output += b));
  server.stderr.on("data", (b) => (output += b));
  await waitFor(
    () =>
      fetch(base + "/health")
        .then((r) => r.ok)
        .catch(() => false),
    "gateway",
    15,
  );
  const c = await fetch(base + "/studio/session", {
    method: "POST",
    headers: { Origin: base },
  }).then((r) => r.json());
  headers = { "Content-Type": "application/json", "x-studio-token": c.token };
}
async function stopServer() {
  if (!server || server.exitCode !== null || server.signalCode !== null) return;
  if (native && headers) await act("stop").catch(() => {});
  const done = new Promise((r) => server.once("exit", r));
  server.kill("SIGINT");
  const fallback = setTimeout(() => server.kill("SIGKILL"), 10000);
  fallback.unref();
  await done;
  clearTimeout(fallback);
}
async function setVllm(patch) {
  const s = await state();
  await act("settings", {
    settings: { ...s.settings, vllm: { ...s.settings.vllm, ...patch } },
  });
}
async function refused(patch, expected, name) {
  await setVllm(patch);
  await act("inspectVllm");
  const diagnostic = (await state()).vllmInspection;
  assert.equal(
    diagnostic.status,
    name === "noEvidenceRejected" ? "unverified" : "blocked",
  );
  if (name === "expiredEvidenceRejected")
    assert.equal(diagnostic.reason, "evidence_expired");
  if (name === "excessContextRejected")
    assert.equal(diagnostic.reason, "evidence_limit_exceeded");
  report.checks[name + "Diagnostic"] = true;
  await act("start");
  await waitFor(async () => {
    const s = await state();
    return s.status === "offline" && s.logs.some((l) => l.includes(expected));
  }, name);
  await act("stop");
  report.checks[name] = true;
}
function tree(pid) {
  const rows = execFileSync("/bin/ps", ["-axo", "pid=,ppid=,stat="], {
    encoding: "utf8",
  })
    .trim()
    .split("\n")
    .map((l) => l.trim().split(/\s+/));
  const ids = new Set([pid]);
  for (let i = 0; i < 10; i++)
    for (const [p, parent] of rows)
      if (ids.has(Number(parent))) ids.add(Number(p));
  return [...ids];
}
function running(ids) {
  const rows = execFileSync("/bin/ps", ["-axo", "pid=,stat="], {
    encoding: "utf8",
  })
    .trim()
    .split("\n")
    .map((l) => l.trim().split(/\s+/));
  return rows.some(
    ([pid, stat]) => ids.includes(Number(pid)) && !stat.startsWith("Z"),
  );
}
try {
  await startServer();
  const initial = await state();
  await act("settings", {
    settings: {
      ...initial.settings,
      language: "en",
      temperature: 0,
      maxTokens: 16,
      python: resolve("Strata/.venv/bin/python"),
      mock: true,
      vllm: {
        root,
        model,
        python,
        executable,
        backend: "mlx_lm",
        evidence: "",
        context: 1024,
      },
    },
  });
  await act("switchEngine", { engine: "vllmApple" });
  await refused(
    { evidence: "" },
    "mlx_lm_version_outside_verified_matrix",
    "noEvidenceRejected",
  );
  const expired = JSON.parse(evidenceBytes);
  expired.architecture_evidence.created_at = 0;
  const expiredPath = join(dir, "expired.json");
  await writeFile(expiredPath, JSON.stringify(expired), { mode: 0o600 });
  await chmod(expiredPath, 0o600);
  await refused(
    { evidence: expiredPath },
    "expired",
    "expiredEvidenceRejected",
  );
  await refused(
    { evidence, context: 2048 },
    "workload exceeds qualified bounds",
    "excessContextRejected",
  );
  await setVllm({ evidence, context: 1024 });
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 960 },
  });
  page.on("pageerror", (e) => consoleErrors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  await page.goto(base);
  assert.equal(await page.title(), "Soliton");
  await preferences(page, "engine");
  assert.equal(
    await page
      .getByRole("textbox", { name: "Qualification evidence JSON" })
      .inputValue(),
    evidence,
  );
  assert.equal(
    await page.getByRole("spinbutton", { name: "Context length" }).inputValue(),
    "1024",
  );
  await page
    .getByRole("button", { name: "Inspect model and evidence", exact: true })
    .click();
  await page
    .locator('[data-inspection-status="verified"]')
    .waitFor({ timeout: 180000 });
  assert.equal(
    (await state()).vllmInspection.reason,
    "identity_bound_text_smoke",
  );
  report.checks.evidenceDiagnosticVerified = true;
  await page.screenshot({ path: "/tmp/soliton-real-inspection.png" });
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await waitFor(
    async () => (await state()).status === "ready",
    "real daemon inference-ready",
  );
  report.checks.inferenceReady = true;
  await page.screenshot({ path: "/tmp/soliton-real-engine.png" });
  await closePreferences(page);
  for (const [locale, prompt, expected] of [
    [
      "English",
      "Answer with exactly this token and nothing else: cobalt",
      "cobalt",
    ],
    ["日本語", "次の語だけを出力してください。説明は不要です：青", "青"],
    ["简体中文", "只输出下列汉字，不要解释：蓝", "蓝"],
  ]) {
    await language(page, locale);
    await closePreferences(page);
    const newConversation = page.locator(
      'button[aria-label="New conversation"],button[aria-label="新しい会話"],button[aria-label="新对话"]',
    );
    await newConversation.click();
    // The backend may finish before the UI fetch/clear-prompt action settles.
    await expect(newConversation).toBeEnabled();
    await waitFor(
      async () => (await state()).document.messages.length === 0,
      "new conversation",
    );
    await page.locator(".message").first().waitFor({ state: "detached" });
    const oldId = (await state()).document.conversations.activeId;
    await page.locator(".composer textarea").fill(prompt);
    await page.locator(".composer button").click();
    const s = await waitFor(async () => {
      const s = await state();
      return !s.document.generating &&
        s.document.messages.at(-1)?.role === "assistant" &&
        s.document.messages.at(-1)?.inference?.metrics.status === "completed"
        ? s
        : null;
    }, `GUI ${locale}`);
    const m = s.document.messages.at(-1);
    assert.equal(m.content.trim(), expected);
    assert.equal(m.inference.metrics.engine, "vLLM-Apple");
    assert(m.inference.metrics.usage.completion > 0);
    report.requests.push({
      locale,
      matched: true,
      metrics: m.inference.metrics,
    });
    assert.equal(s.document.conversations.activeId, oldId);
  }
  await page.screenshot({ path: "/tmp/soliton-real-chat.png" });
  report.checks.multilingualGui = true;
  const api = await fetch(base + "/v1/chat/completions", {
    method: "POST",
    headers,
    body: JSON.stringify({
      model: "default_model",
      messages: [
        { role: "user", content: "What is 2+2? Answer only the digit." },
      ],
      max_tokens: 8,
      temperature: 0,
    }),
  });
  assert(api.ok, await api.clone().text());
  const apiBody = await api.json();
  assert.equal(apiBody.choices[0].message.content.trim(), "4");
  report.checks.nonstreamApi = true;
  const beforeRejectedChat = (await state()).document;
  const tooLong = "hello ".repeat(2048);
  for (const [path, messages, max_tokens, code] of [
    [
      "/v1/chat/completions",
      [{ role: "user", content: tooLong }],
      64,
      "context_length_exceeded",
    ],
    [
      "/studio/chat",
      [{ role: "user", content: tooLong }],
      64,
      "context_length_exceeded",
    ],
    [
      "/v1/chat/completions",
      [{ role: "user", content: "hello" }],
      undefined,
      "invalid_request_token_budget",
    ],
  ]) {
    const rejected = await fetch(base + path, {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: "default_model",
        messages,
        max_tokens,
        stream: false,
      }),
    });
    assert.equal(rejected.status, 422);
    assert.equal((await rejected.json()).error.code, code);
  }
  assert.deepEqual((await state()).document, beforeRejectedChat);
  report.checks.exactContextRejectionAndHistory = true;

  await waitFor(
    async () => (await state()).memoryUsage.engine.current?.pid,
    "native process sample",
  );
  const current = await state();
  const ids = tree(current.memoryUsage.engine.current.pid);
  assert(ids.length >= 2);
  report.memorySample = current.memoryUsage.engine.current;
  const c = new AbortController();
  const stream = await fetch(base + "/v1/chat/completions", {
    method: "POST",
    headers,
    signal: c.signal,
    body: JSON.stringify({
      model: "default_model",
      messages: [
        {
          role: "user",
          content: "List the numbers from 1 to 100, one per line.",
        },
      ],
      max_tokens: 256,
      temperature: 0,
      stream: true,
    }),
  });
  if (!stream.ok) assert.fail(await stream.text());
  const reader = stream.body.getReader();
  await reader.read();
  c.abort();
  await reader.cancel().catch(() => {});
  await waitFor(async () => {
    const s = await state();
    return s.requests.some(
      (m) => m.source === "api" && m.status === "cancelled",
    );
  }, "cancel metrics");
  report.checks.cancel = true;
  await act("switchEngine", { engine: "strata" });
  assert(!(await state()).health);
  assert.equal(running(ids), false);
  const before = (await state()).document.messages;
  await act("start");
  await waitFor(
    async () => (await state()).status === "ready",
    "Strata mock ready",
  );
  assert.deepEqual((await state()).document.messages, before);
  await act("switchEngine", { engine: "vllmApple" });
  await act("start");
  await waitFor(
    async () => (await state()).status === "ready",
    "Apple restart with verified evidence",
  );
  report.checks.stopSwitchRestart = true;
  await act("stop");
  await page.close();
  await stopServer();
  await startServer();
  const restored = await state();
  assert.equal(restored.status, "offline");
  assert.equal(restored.settings.vllm.evidence, evidence);
  assert.deepEqual(restored.document.messages, before);
  assert(restored.document.conversations.items.length >= 3);
  report.checks.persistence = true;
  assert.deepEqual(consoleErrors, []);
  report.checks.console = true;
  assert.equal(
    createHash("sha256")
      .update(await readFile(evidence))
      .digest("hex"),
    report.evidenceSha256,
  );
  report.passed = true;
} catch (error) {
  report.failure = { message: error.message.split("\n")[0] };
  if (headers) {
    const s = await state().catch(() => null);
    if (s) report.failure.status = s.status;
  }
  throw error;
} finally {
  await stopServer();
  if (browser) await browser.close();
  report.finishedAt = new Date().toISOString();
  await writeFile(
    process.env.SOLITON_REAL_REPORT || "/tmp/soliton-real-model-report.json",
    JSON.stringify(report, null, 2) + "\n",
  );
  await rm(dir, { recursive: true, force: true });
}
console.log(
  "PASS: real vLLM-Apple through Soliton, evidence rejection/acceptance, EN/JA/ZH GUI, API, cancel, worker cleanup, switch/restart/persistence",
);
