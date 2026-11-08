// Real qualified shared-core lifecycle. Dedicated offline QA host/state required.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile, writeFile, unlink } from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { dirname, join, isAbsolute } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { cases } from "./benchmark-engine-gateway.mjs";
const args = process.argv.slice(2),
  value = (n) => args[args.indexOf(n) + 1];
assert(
  args.includes("--isolated") &&
    args.includes("--api") &&
    args.includes("--output") &&
    args.includes("--strata-root") &&
    args.includes("--strata-python"),
);
assert(
  isAbsolute(value("--strata-root")) && isAbsolute(value("--strata-python")),
);
const api = value("--api"),
  output = value("--output");
assert.equal(new URL(api).hostname, "127.0.0.1");
assert(!["8080", "8081", ""].includes(new URL(api).port));
await writeFile(output, "", { flag: "wx", mode: 0o600 });
const { token } = await fetch(api + "/studio/session", {
  method: "POST",
  headers: { Origin: api },
}).then((r) => r.json());
const headers = { "Content-Type": "application/json", "x-studio-token": token };
async function request(path, body, status = 200) {
  const r = await fetch(api + path, {
    method: body === undefined ? "GET" : "POST",
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(120000),
  });
  assert.equal(r.status, status, `${path}: HTTP ${r.status}`);
  return r;
}
const state = () => request("/studio/state").then((r) => r.json()),
  action = (action, data = {}, status = 200) =>
    request("/studio/action", { action, ...data }, status).then((r) =>
      r.json(),
    );
async function until(test, label) {
  for (let i = 0; i < 300; i++) {
    const s = await state();
    if (test(s)) return s;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw Error("Timed out: " + label);
}
const processes = () =>
  execFileSync("/bin/ps", ["-axo", "pid=,ppid=,command="], {
    encoding: "utf8",
    maxBuffer: 8 * 1024 ** 2,
  })
    .split("\n")
    .map((line) => line.match(/^\s*(\d+)\s+(\d+)\s+(.*)$/))
    .filter(Boolean)
    .map((m) => ({ pid: +m[1], parent: +m[2], command: m[3] }));
function tree(pid) {
  const rows = processes();
  let ids = new Set([pid]);
  for (let i = 0; i < 32; i++) {
    const next = new Set([
      ...ids,
      ...rows.filter((r) => ids.has(r.parent)).map((r) => r.pid),
    ]);
    if (next.size === ids.size) return [...ids];
    ids = next;
  }
  throw Error("Unbounded process tree");
}
async function gone(ids) {
  for (let i = 0; i < 100; i++) {
    if (!processes().some((p) => ids.includes(p.pid))) return;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw Error("Owned child survived stop/switch");
}
const initial = await state();
assert.equal(initial.status, "offline");
assert.equal(initial.settings.engine, "vllmApple");
assert.equal(initial.settings.vllm.context, 4096);
assert.equal(initial.settings.vllm.kvCacheMiB, 1536);
assert.deepEqual(initial.document.messages, []);
const settings = initial.settings,
  report = {
    result: "running",
    scope:
      "Qualified headless Rust API lifecycle; Strata switch uses mock, no native GUI/OS-drag claim",
    checks: {},
    refusals: [],
  };
let owned = false,
  invalidPath,
  activeAbort;
const proofBytes = await readFile(settings.vllm.evidence),
  proof = JSON.parse(proofBytes);
assert.equal(proof.architecture_evidence.scope, "text_smoke_30min");
assert.equal(proof.architecture_evidence.context_tokens, 4096);
assert.equal(proof.backend_versions.mlx_lm, "0.32.2");
report.evidenceSha256 = createHash("sha256").update(proofBytes).digest("hex");
async function refused(vllm, label) {
  if (label === "missing-evidence") {
    // A nonzero KV/allocator policy already requires evidence at settings admission.
    // Do not start the unchanged valid profile after that earlier refusal.
    await action(
      "settings",
      {
        settings: { ...settings, vllm: { ...settings.vllm, ...vllm } },
      },
      400,
    );
    const s = await state();
    assert.equal(s.status, "offline");
    assert.deepEqual(s.settings, settings);
    assert.equal(s.requests.length, initial.requests.length);
    report.refusals.push({
      label,
      stage: "settings",
      httpStatus: 400,
      noInference: true,
    });
    return;
  }
  await action("settings", {
    settings: { ...settings, vllm: { ...settings.vllm, ...vllm } },
  });
  owned = true;
  await action("start");
  const s = await until((s) => s.status === "offline", label);
  assert(s.startup?.counterSpawnedMs == null);
  assert.equal(s.requests.length, initial.requests.length);
  await action("stop");
  owned = false;
  report.refusals.push({
    label,
    offline: true,
    noCounterObserved: true,
    noInference: true,
  });
}
const fixture = cases()[0],
  body = {
    model: "default_model",
    messages: fixture.messages,
    max_tokens: fixture.maxTokens,
    temperature: 0,
  };
try {
  await refused({ evidence: "" }, "missing-evidence");
  const expired = structuredClone(proof);
  expired.architecture_evidence.created_at = 0;
  invalidPath = join(dirname(output), `invalid-expired-${randomUUID()}.json`);
  await writeFile(invalidPath, JSON.stringify(expired), {
    flag: "wx",
    mode: 0o600,
  });
  await refused({ evidence: invalidPath }, "expired-evidence");
  await refused({ context: 4097 }, "beyond-qualified-context");
  await action("settings", {
    settings: {
      ...settings,
      mock: true,
      python: value("--strata-python"),
      strataRoot: value("--strata-root"),
      config: "",
    },
  });
  owned = true;
  await action("start");
  let ready = await until((s) => s.status === "ready", "qualified-start");
  const session = ready.engineSession;
  assert(session);
  const answer = await request("/v1/chat/completions", body).then((r) =>
    r.json(),
  );
  assert.equal(answer.choices[0].message.content.trim(), fixture.exact);
  report.checks.jsonAnswer = true;
  const beforeInvalid = await state();
  const previous = new Set(beforeInvalid.requests.map((r) => r.id));
  for (const invalid of [
    {
      ...body,
      messages: [
        {
          role: "user",
          content: [
            {
              type: "image_url",
              image_url: { url: "http://127.0.0.1:1/must-not-fetch" },
            },
          ],
        },
      ],
    },
    { ...body, tools: [] },
  ]) {
    const refused = await request("/v1/chat/completions", invalid, 422).then(
      (r) => r.json(),
    );
    assert.equal(refused.error.code, "invalid_request_token_budget");
  }
  const invalidState = await until(
    (s) =>
      s.requests.filter(
        (r) => !previous.has(r.id) && r.gateway?.admissionRejected === true,
      ).length === 2,
    "plain-text admission refusal metrics",
  );
  assert(
    invalidState.requests
      .filter((r) => !previous.has(r.id))
      .every((r) => r.gateway?.dispatchMs == null),
  );
  report.checks.plainTextAdmissionMaintained = true;
  const before = await state();
  const beforeIds = new Set(before.requests.map((r) => r.id));
  const abort = new AbortController();
  activeAbort = abort;
  const stream = await fetch(api + "/v1/chat/completions", {
    method: "POST",
    headers,
    signal: AbortSignal.any([abort.signal, AbortSignal.timeout(120000)]),
    body: JSON.stringify({
      ...body,
      messages: [
        {
          role: "user",
          content: "List the numbers from 1 to 100, one per line.",
        },
      ],
      max_tokens: 256,
      stream: true,
    }),
  });
  assert(stream.ok);
  const reader = stream.body.getReader();
  const decoder = new TextDecoder();
  let content = "";
  for (let i = 0; i < 100 && !content.includes('"content":'); i++) {
    const chunk = await reader.read();
    assert(!chunk.done);
    content += decoder.decode(chunk.value, { stream: true });
  }
  assert(content.includes('"content":'));
  const busy = await action("switchEngine", { engine: "strata" }, 400);
  assert.equal(
    busy.error.message,
    "Wait for active requests and model preparation to finish",
  );
  report.checks.activeSwitchRefused = true;
  abort.abort();
  await reader.cancel().catch(() => {});
  await until(
    (s) =>
      s.requests.some((r) => !beforeIds.has(r.id) && r.status === "cancelled"),
    "cancelled-metric",
  );
  report.checks.cancel = true;
  ready = await until(
    (s) => s.memoryUsage.engine.current?.pid,
    "owned-tree-sample",
  );
  const enginePid = ready.memoryUsage.engine.current.pid;
  const rows = processes(),
    host = rows.find((p) => p.pid === enginePid)?.parent;
  assert(host);
  const ids = [
    ...tree(enginePid),
    ...rows
      .filter(
        (p) =>
          p.parent === host && p.command.includes("--soliton-token-counter"),
      )
      .map((p) => p.pid),
  ];
  await action("switchEngine", { engine: "strata" });
  owned = false;
  await gone(ids);
  assert.equal((await state()).engineSession, null);
  report.checks.ownedTreeAndCounterGone = true;
  owned = true;
  await action("start");
  await until((s) => s.status === "ready", "strata-mock-ready");
  report.checks.strataMockSwitch = true;
  await action("switchEngine", { engine: "vllmApple" });
  owned = false;
  owned = true;
  await action("start");
  ready = await until((s) => s.status === "ready", "mlx-restart");
  assert(ready.engineSession && ready.engineSession !== session);
  report.checks.sessionChangedOnRestart = true;
  const recovered = await request("/v1/chat/completions", body).then((r) =>
    r.json(),
  );
  assert.equal(recovered.choices[0].message.content.trim(), fixture.exact);
  report.checks.restartAnswer = true;
  assert.deepEqual((await state()).document, initial.document);
  report.checks.apiHistoryPreserved = true;
  const chatRaw = await request("/studio/chat", { ...body, stream: true }).then(
    (r) => r.text(),
  );
  assert(chatRaw.includes("[DONE]"));
  let chatAnswer = "";
  for (const line of chatRaw.split("\n")) {
    if (!line.startsWith("data:") || line.includes("[DONE]")) continue;
    const frame = JSON.parse(line.slice(5));
    for (const choice of frame.choices || [])
      chatAnswer += choice.delta?.content || "";
  }
  assert.equal(chatAnswer.trim(), fixture.exact);
  const chat = await until(
    (s) =>
      !s.document.generating &&
      s.document.messages.at(-1)?.inference?.metrics.status === "completed",
    "chat-completed",
  );
  assert.equal(chat.document.messages.length, 2);
  assert.equal(chat.document.messages.at(-1).content.trim(), fixture.exact);
  report.checks.chatSseAndMetrics = true;
  if (args.includes("--state-path")) {
    let persisted = false;
    for (let i = 0; i < 100; i++) {
      const saved = JSON.parse(await readFile(value("--state-path")));
      if (isDeepStrictEqual(saved.document.messages, chat.document.messages)) {
        persisted = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 50));
    }
    assert(persisted, "QA chat/metrics did not reach the storage file");
    report.checks.chatSavedToDisk = true;
  }
  await action("stop");
  owned = false;
  assert.equal((await state()).engineSession, null);
  assert.deepEqual((await state()).document, chat.document);
  assert.deepEqual(await readFile(settings.vllm.evidence), proofBytes);
  report.checks.chatPreservedOnStopAndEvidenceUnchanged = true;
  report.result = "passed";
} catch (error) {
  report.result = "failed";
  report.error = String(error);
  throw error;
} finally {
  activeAbort?.abort();
  try {
    if (owned) await action("stop");
  } finally {
    try {
      await action("settings", { settings });
      if (invalidPath) await unlink(invalidPath);
    } finally {
      await writeFile(output, JSON.stringify(report, null, 2) + "\n", {
        mode: 0o600,
      });
    }
  }
}
console.log(
  JSON.stringify({
    result: report.result,
    checks: report.checks,
    refusals: report.refusals.length,
  }),
);
