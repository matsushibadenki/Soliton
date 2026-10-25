// Real-model before/after measurement. No downloads or user-state writes.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir, loadavg } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";
import { stableOwnedRss } from "./owned-memory-sample.mjs";
import { makeCases, options } from "./benchmark-inference.mjs";
const [label, binary, output] = process.argv.slice(2);
assert(
  label && binary && output,
  "node scripts/token-count-benchmark.mjs before|after BINARY NEW_REPORT",
);
const source =
  process.env.SOLITON_NATIVE_STATE ||
  "/Users/littlebuddha/Library/Application Support/dev.soliton.desktop/studio.json";
const original = await readFile(source);
const seed = JSON.parse(original);
seed.settings.engine = "vllmApple";
seed.settings.mock = true;
seed.settings.strataRoot = resolve("Strata");
seed.settings.python = resolve("Strata/.venv/bin/python");
await readFile(resolve("Strata/serve/server.py"));
await readFile(seed.settings.python);
if (process.env.SOLITON_REAL_EVIDENCE)
  seed.settings.vllm.evidence = resolve(process.env.SOLITON_REAL_EVIDENCE);
const evidence = await readFile(seed.settings.vllm.evidence);
const hash = (v) => createHash("sha256").update(v).digest("hex");
const headless = process.env.SOLITON_BENCHMARK_RUNTIME === "headless";
const port = headless ? "18084" : "8080";
const base = `http://127.0.0.1:${port}`;
assert.equal(
  await fetch(base + "/health")
    .then(() => true)
    .catch(() => false),
  false,
  "port occupied",
);
const engineRows = () =>
  execFileSync("/bin/ps", ["-axo", "pid=,ppid=,stat=,command="], {
    encoding: "utf8",
  })
    .split("\n")
    .filter(
      (l) =>
        l.includes("/mlx_lm.server --model ") ||
        (l.includes("/mlx_kv_budget.py") && l.includes(" --model ")) ||
        (l.includes("from vllm_apple.cli import main;") &&
          l.includes(" serve ")),
    );
assert.equal(engineRows().length, 0, "an inference process already exists");
const dir = await mkdtemp(join(tmpdir(), "soliton-token-benchmark-"));
await writeFile(join(dir, "state.json"), JSON.stringify(seed), { mode: 0o600 });
let app, headers;
const report = {
  schema: 1,
  passed: false,
  environment: { architecture: process.arch, loadAverageAtStart: loadavg() },
  binarySha256: hash(await readFile(resolve(binary))),
  label,
  runtime: headless ? "shared Rust headless core" : "native Tauri",
  startedAt: new Date().toISOString(),
  evidenceSha256: hash(evidence),
  context: seed.settings.vllm.context,
  kvCacheMiB: seed.settings.vllm.kvCacheMiB ?? 0,
  allocatorCacheMiB: seed.settings.vllm.allocatorCacheMiB ?? 0,
  settingsFingerprint: hash(JSON.stringify(seed.settings.vllm)),
  cases: [],
  checks: {},
};
const state = () =>
  fetch(base + "/studio/state", { headers }).then((r) => r.json());
const act = async (action, data = {}) => {
  const r = await fetch(base + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...data }),
  });
  assert(r.ok, await r.clone().text());
};
const wait = async (fn) => {
  for (let i = 0; i < 450; i++) {
    const v = await fn();
    if (v) return v;
    await new Promise((r) => setTimeout(r, 400));
  }
  throw Error("timeout");
};
function counterPids() {
  return execFileSync("/bin/ps", ["-axo", "pid=,ppid="], { encoding: "utf8" })
    .trim()
    .split("\n")
    .map((l) => l.trim().split(/\s+/).map(Number))
    .filter(([, parent]) => parent === app.pid)
    .map(([pid]) => pid)
    .filter((pid) => {
      try {
        return execFileSync("/bin/ps", ["-p", String(pid), "-o", "command="], {
          encoding: "utf8",
        }).includes("--soliton-token-counter");
      } catch {
        return false;
      }
    });
}
const gone = (pid) => {
  try {
    const v = execFileSync("/bin/ps", ["-p", String(pid), "-o", "stat="], {
      encoding: "utf8",
    }).trim();
    return !v || v.startsWith("Z");
  } catch {
    return true;
  }
};
function descendants() {
  const rows = execFileSync("/bin/ps", ["-axo", "pid=,ppid="], {
    encoding: "utf8",
  })
    .trim()
    .split("\n")
    .map((l) => l.trim().split(/\s+/).map(Number));
  const ids = new Set([app.pid]);
  for (let i = 0; i < 10; i++)
    for (const [p, pp] of rows) if (ids.has(pp)) ids.add(p);
  return [...ids].filter((p) => p !== app.pid);
}
try {
  app = spawn(resolve(binary), [], {
    env: {
      ...process.env,
      STUDIO_PORT: port,
      STUDIO_STATE_PATH: join(dir, "state.json"),
    },
    stdio: ["ignore", "ignore", "ignore"],
  });
  app.once("exit", (code, signal) => {
    report.applicationExit = { code, signal };
  });
  await wait(() =>
    fetch(base + "/health")
      .then((r) => r.ok)
      .catch(() => false),
  );
  const session = await fetch(base + "/studio/session", {
    method: "POST",
    headers: { Origin: base },
  }).then((r) => r.json());
  headers = {
    "Content-Type": "application/json",
    "x-studio-token": session.token,
  };
  const before = await state();
  const engineStarted = performance.now();
  await act("start");
  report.startupObservations = [];
  let previousPhase;
  const readyState = await wait(async () => {
    const snapshot = await state();
    const control = snapshot.health?.control_ready === true;
    const phase = `${snapshot.status}/${control}`;
    if (phase !== previousPhase) {
      const counters = counterPids().length;
      report.startupObservations.push({
        elapsedMs: performance.now() - engineStarted,
        phase,
        counterProcesses: counters,
      });
      if (!control && snapshot.status === "starting")
        assert.equal(counters, 0, "counter started before qualification");
      previousPhase = phase;
    }
    if (snapshot.status === "offline") {
      report.startupRejection = "engine_offline_before_ready";
      throw Error("engine startup refused before readiness");
    }
    return snapshot.status === "ready" ? snapshot : false;
  });
  report.startup = readyState.startup ?? null;
  if (process.env.SOLITON_EXPECT_PREWARM === "1") {
    assert.equal(counterPids().length, 1);
    assert(
      report.startup.counterSpawnedMs >= report.startup.controlReadyObservedMs,
    );
    assert(
      report.startup.counterSpawnedMs < report.startup.inferenceReadyObservedMs,
    );
    report.checks.counterOverlapsModelLoadingAfterQualification = true;
  }
  report.engineStartupMs = performance.now() - engineStarted;
  if (report.kvCacheMiB) {
    const rows = execFileSync(
      "/bin/ps",
      ["-p", descendants().join(","), "-o", "command="],
      { encoding: "utf8" },
    );
    assert(
      rows.includes(`--prompt-cache-bytes ${report.kvCacheMiB * 1024 ** 2}`),
    );
    report.checks.backendKvBudgetArgument = true;
  }
  if (process.env.SOLITON_FIRST_STREAM === "1") {
    const started = performance.now();
    const response = await fetch(base + "/v1/chat/completions", {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: "default_model",
        messages: [
          {
            role: "user",
            content: "Answer with exactly this token and nothing else: cobalt",
          },
        ],
        max_tokens: 16,
        temperature: 0,
        stream: true,
      }),
      signal: AbortSignal.timeout(90000),
    });
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      report.coldStream = {
        status: response.status,
        elapsedMs: performance.now() - started,
        errorCode: [
          "tokenization_unavailable",
          "context_length_exceeded",
        ].includes(body.error?.code)
          ? body.error.code
          : "unclassified",
      };
    }
    assert(response.ok, `stream HTTP ${response.status}`);
    let buffer = "",
      text = "",
      firstContentMs;
    const decoder = new TextDecoder();
    for await (const chunk of response.body) {
      buffer += decoder.decode(chunk, { stream: true });
      let newline;
      while ((newline = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (!line.startsWith("data:") || line === "data: [DONE]") continue;
        const frame = JSON.parse(line.slice(5));
        const content = frame.choices?.[0]?.delta?.content;
        if (content) {
          firstContentMs ??= performance.now() - started;
          text += content;
        }
      }
    }
    assert.equal(text.trim(), "cobalt");
    assert(Number.isFinite(firstContentMs));
    report.coldStream = {
      firstContentMs,
      completedMs: performance.now() - started,
      engineStartToFirstContentMs: report.engineStartupMs + firstContentMs,
      exact: true,
    };
  }
  const cases = makeCases(
    options(["--suite", "workload", "--rows", "0,32,58,64"]),
  );
  for (let round = 0; round < 2; round++)
    for (const item of cases) {
      const started = performance.now();
      const r = await fetch(base + "/v1/chat/completions", {
        method: "POST",
        headers,
        body: JSON.stringify({
          model: "default_model",
          messages: item.messages,
          max_tokens: 64,
          temperature: 0,
          stream: false,
          chat_template_kwargs: { enable_thinking: false },
        }),
        signal: AbortSignal.timeout(90000),
      });
      const body = await r.json();
      const elapsedMs = performance.now() - started;
      const row = {
        round,
        language: item.language,
        kind: item.kind,
        rows: item.rows,
        status: r.status,
        elapsedMs: Math.round(elapsedMs * 1000) / 1000,
      };
      if (item.rows === 64 && report.context === 1024) {
        assert.equal(r.status, 422);
        assert.equal(body.error.code, "context_length_exceeded");
        row.rejectionCode = body.error.code;
      } else {
        assert.equal(r.status, 200);
        assert.equal(body.choices[0].message.content.trim(), item.expected);
        row.exact = true;
        row.usage = body.usage;
      }
      report.cases.push(row);
      console.error(
        `${label}: ${report.cases.length}/30 ${item.language}/${item.rows ?? item.kind} HTTP ${r.status}`,
      );
    }
  if (report.coldStream && report.context === 1024) {
    const boundaryIndex = cases.findIndex(
      (c) => c.language === "en" && c.rows === 58,
    );
    assert(boundaryIndex >= 0);
    const promptTokens = report.cases[boundaryIndex].usage.prompt_tokens;
    for (const extra of [0, 1]) {
      const response = await fetch(base + "/v1/chat/completions", {
        method: "POST",
        headers,
        body: JSON.stringify({
          model: "default_model",
          messages: cases[boundaryIndex].messages,
          max_tokens: seed.settings.vllm.context - promptTokens + extra,
          temperature: 0,
          stream: false,
          chat_template_kwargs: { enable_thinking: false },
        }),
        signal: AbortSignal.timeout(90000),
      });
      const body = await response.json();
      assert.equal(response.status, extra ? 422 : 200);
      if (extra) assert.equal(body.error.code, "context_length_exceeded");
      else {
        assert.equal(body.usage.prompt_tokens, promptTokens);
        assert.equal(
          body.choices[0].message.content.trim(),
          cases[boundaryIndex].expected,
        );
      }
    }
    report.checks.exactContextBoundary = true;
  }
  const sampleMemory = async () => {
    const rss = await stableOwnedRss({
      descendants,
      readRows: (ids) =>
        execFileSync("/bin/ps", ["-p", ids.join(","), "-o", "pid=,rss="], {
          encoding: "utf8",
        })
          .trim()
          .split("\n")
          .map((line) => line.trim().split(/\s+/).map(Number)),
    });
    let kvCache = null;
    if (report.kvCacheMiB) {
      const commandRows = execFileSync(
        "/bin/ps",
        ["-p", descendants().join(","), "-o", "command="],
        { encoding: "utf8" },
      ).split("\n");
      const backends = commandRows.filter(
        (line) =>
          line.includes("/mlx_kv_budget.py") && line.includes(" --model "),
      );
      assert.equal(backends.length, 1, "owned KV wrapper missing or ambiguous");
      const match = backends[0].match(/--host 127\.0\.0\.1 --port (\d+) /);
      assert(match, "owned backend loopback port missing");
      const response = await fetch(
        `http://127.0.0.1:${match[1]}/vllm-apple/kv-cache`,
        { signal: AbortSignal.timeout(5000) },
      );
      assert.equal(response.status, 200);
      assert(Number(response.headers.get("content-length")) <= 4096);
      const raw = await response.text();
      assert(raw.length <= 4096);
      kvCache = JSON.parse(raw);
      assert.equal(kvCache.max_bytes, report.kvCacheMiB * 1024 ** 2);
      if (report.allocatorCacheMiB) {
        assert.equal(
          kvCache.allocator.configured_limit_bytes,
          report.allocatorCacheMiB * 1024 ** 2,
        );
        assert(
          kvCache.allocator.effective_limit_bytes <=
            kvCache.allocator.configured_limit_bytes,
        );
        report.checks.backendAllocatorBudget = true;
      }
      assert(
        Number.isSafeInteger(kvCache.retained_bytes) &&
          kvCache.retained_bytes >= 0 &&
          kvCache.retained_bytes <= kvCache.max_bytes,
      );
    }
    const snapshot = await state();
    assert.equal(snapshot.status, "ready");
    return {
      elapsedMs: performance.now() - engineStarted,
      ...rss,
      kvCache,
      enginePhysicalFootprint: snapshot.memoryUsage?.engine?.current ?? null,
      systemAvailableBytes:
        snapshot.memoryUsage?.current?.availableBytes ?? null,
    };
  };
  if (process.env.SOLITON_CONTEXT_MATRIX === "1") {
    const { verifyContextMatrix } = await import("./context-matrix.mjs");
    report.contextMatrix = await verifyContextMatrix({
      base,
      headers,
      context: report.context,
      sampleMemory,
    });
  }
  if (process.env.SOLITON_CACHE_MEMORY === "1") {
    const { verifyCacheMemory } = await import("./cache-memory-workload.mjs");
    report.cacheMemory = await verifyCacheMemory({
      base,
      headers,
      context: report.context,
      sampleMemory,
      recentReuseRequired: !report.kvCacheMiB,
      verifySeeded: report.kvCacheMiB > 0,
    });
  }
  if (report.contextMatrix || report.cacheMemory) {
    assert(
      report.contextMatrix?.passed || report.cacheMemory?.boundary?.passed,
    );
    report.checks.exactContextBoundary = true;
    const snapshot = await state();
    assert(snapshot.requests.length > 0 && snapshot.requests.length <= 100);
    assert(
      snapshot.requests.every(
        (row) => row.engine === "vLLM-Apple" && row.source === "api",
      ),
    );
    const long = snapshot.requests.filter(
      (row) =>
        Number.isSafeInteger(row.usage?.prompt) &&
        row.usage.prompt >= report.context - 512,
    );
    assert(long.length > 0, "shared telemetry lost long-prompt usage");
    if (report.cacheMemory) {
      const cached = long.map((row) => row.usage?.cached);
      assert(cached.every((n) => Number.isSafeInteger(n) && n >= 0));
      assert(cached.some((n) => n >= 3500) && cached.some((n) => n < 64));
      report.cacheMemory.sharedCachedTelemetry = {
        verified: true,
        min: Math.min(...cached),
        max: Math.max(...cached),
      };
    }
    report.sharedRequestMetrics = {
      retained: snapshot.requests.length,
      longPromptUsageRecords: long.length,
      engine: "vLLM-Apple",
      bounded: true,
    };
  }
  const counters = counterPids();
  report.counterProcessesAfterRequests = counters.length;
  const tree = descendants();
  report.counterRssKiB = counters.map((p) =>
    Number(
      execFileSync("/bin/ps", ["-p", String(p), "-o", "rss="], {
        encoding: "utf8",
      }).trim(),
    ),
  );
  if (label === "after") {
    assert.equal(counters.length, 1);
    process.kill(counters[0], "SIGKILL");
    for (let attempt = 0; attempt < 2; attempt++) {
      const rejected = await fetch(base + "/v1/chat/completions", {
        method: "POST",
        headers,
        body: JSON.stringify({
          model: "default_model",
          messages: [{ role: "user", content: "hello" }],
          max_tokens: 16,
        }),
      });
      assert.equal(rejected.status, 503);
      assert.equal(
        (await rejected.json()).error.code,
        "tokenization_unavailable",
      );
    }
    assert.equal(counterPids().length, 0);
    report.checks.counterFailureClosedAndLatched = true;
  }
  await act("stop");
  assert(counters.every(gone));
  assert(tree.every(gone));
  assert.equal(engineRows().length, 0);
  report.checks.stopReapsCounter = true;
  report.checks.stopReapsWholeTree = true;
  if (report.cacheMemory) {
    const stopped = await state();
    assert.equal(stopped.status, "offline");
    assert.equal(stopped.memoryUsage?.engine?.current, null);
    report.cacheMemory.afterStop = {
      enginePhysicalFootprint: null,
      ownedTreeGone: true,
      systemAvailableBytes:
        stopped.memoryUsage?.current?.availableBytes ?? null,
    };
  }
  await act("switchEngine", { engine: "strata" });
  assert.equal(counterPids().length, 0);
  await act("start");
  await wait(async () => (await state()).status === "ready");
  assert.equal(counterPids().length, 0);
  assert.equal(engineRows().length, 0);
  await act("stop");
  await act("switchEngine", { engine: "vllmApple" });
  report.checks.noCounterWithStrata = true;
  const after = await state();
  try {
    assert.deepEqual(after.document, before.document);
  } catch {
    report.interferenceDetected = true;
    throw Error(
      "Document changed outside the benchmark; private isolated state will be retained for recovery",
    );
  }
  assert.deepEqual(after.settings, before.settings);
  assert.equal(hash(await readFile(source)), hash(original));
  assert.equal(
    hash(await readFile(seed.settings.vllm.evidence)),
    hash(evidence),
  );
  report.checks.stateAndEvidencePreserved = true;
  if (label === "after") {
    await act("start");
    await wait(async () => (await state()).status === "ready");
    const r = await fetch(base + "/v1/chat/completions", {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: "default_model",
        messages: [
          {
            role: "user",
            content: "Answer with exactly this token and nothing else: cobalt",
          },
        ],
        max_tokens: 16,
        temperature: 0,
      }),
    });
    assert(r.ok);
    await r.json();
    const tree = descendants();
    assert.equal(counterPids().length, 1);
    const exit = new Promise((resolve) => app.once("exit", resolve));
    app.kill(headless ? "SIGINT" : "SIGTERM");
    let timer;
    try {
      await Promise.race([
        exit,
        new Promise((_, reject) => {
          timer = setTimeout(
            () => reject(Error("signal shutdown timed out")),
            20000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
    await wait(async () => tree.every(gone));
    assert.equal(engineRows().length, 0);
    report.checks[
      headless ? "headlessSignalReapsTree" : "nativeSignalReapsTree"
    ] = true;
    report.checks.restartAfterCounterFailure = true;
  }
  report.passed = true;
  const accepted = report.cases.filter((c) => c.exact);
  const warm = accepted
    .slice(1)
    .map((c) => c.elapsedMs)
    .sort((a, b) => a - b);
  report.summary = {
    accepted: accepted.length,
    rejected: report.cases.length - accepted.length,
    firstAcceptedMs: accepted[0].elapsedMs,
    warmMedianMs: warm[Math.floor(warm.length / 2)],
    warmMeanMs: warm.reduce((a, b) => a + b, 0) / warm.length,
  };
  console.log(
    JSON.stringify({ label, summary: report.summary, checks: report.checks }),
  );
} finally {
  if (app?.exitCode === null) {
    if (headers) await act("stop").catch(() => {});
    const done = new Promise((r) => app.once("exit", r));
    app.kill("SIGINT");
    const timer = setTimeout(() => app.kill("SIGKILL"), 10000);
    await done;
    clearTimeout(timer);
  }
  report.environment.loadAverageAtEnd = loadavg();
  report.finishedAt = new Date().toISOString();
  await writeFile(output, JSON.stringify(report, null, 2) + "\n", {
    flag: "wx",
    mode: 0o600,
  });
  if (report.interferenceDetected)
    console.error(`Private recovery state retained at ${dir}`);
  else await rm(dir, { recursive: true, force: true });
}
