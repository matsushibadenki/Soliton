// Isolated real Soliton startup refusals. Never starts a qualified inference model.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";
const [binary, source, oldEvidence, newEvidence, output] =
  process.argv.slice(2);
assert(output, "BINARY SOURCE OLD_EVIDENCE NEW_EVIDENCE NEW_REPORT");
const base = "http://127.0.0.1:18084";
const original = await readFile(source),
  seed = JSON.parse(original);
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const evidence = await Promise.all(
  [oldEvidence, newEvidence].map((path) => readFile(path)),
);
const forbidden = () =>
  execFileSync("/bin/ps", ["-axo", "pid=,command="], { encoding: "utf8" })
    .split("\n")
    .filter(
      (line) =>
        line.includes("/mlx_lm.server --model ") ||
        (line.includes("/mlx_kv_budget.py") && line.includes(" --model ")) ||
        line.includes("--soliton-token-counter"),
    );
assert.equal(
  forbidden().length,
  0,
  "another inference/counter process is present",
);
assert.equal(
  await fetch(base + "/health")
    .then(() => true)
    .catch(() => false),
  false,
);
seed.settings.engine = "vllmApple";
const dir = await mkdtemp(join(tmpdir(), "soliton-context-gates-"));
await writeFile(join(dir, "state.json"), JSON.stringify(seed), { mode: 0o600 });
const report = {
  passed: false,
  checks: [],
  sourceSha256: hash(original),
  evidenceSha256: evidence.map(hash),
};
let app, headers;
const state = () =>
  fetch(base + "/studio/state", { headers }).then((r) => r.json());
const act = async (action, data = {}) => {
  const response = await fetch(base + "/studio/action", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...data }),
  });
  assert(response.ok, `action HTTP ${response.status}`);
};
const wait = async (fn) => {
  for (let n = 0; n < 150; n++) {
    if (await fn()) return;
    await new Promise((r) => setTimeout(r, 400));
  }
  throw Error("startup refusal timeout");
};
try {
  app = spawn(resolve(binary), [], {
    env: {
      ...process.env,
      STUDIO_PORT: "18084",
      STUDIO_STATE_PATH: join(dir, "state.json"),
    },
    stdio: "ignore",
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
  const trials =
    process.env.SOLITON_ALLOCATOR_GATES === "1"
      ? [
          [
            4096,
            newEvidence,
            1024,
            "evidence_identity_mismatch",
            "identity mismatch",
            0,
          ],
          [
            4096,
            newEvidence,
            1024,
            "evidence_identity_mismatch",
            "identity mismatch",
            128,
          ],
          [
            4097,
            newEvidence,
            1024,
            "evidence_limit_exceeded",
            "workload exceeds qualified bounds",
            256,
          ],
        ]
      : process.env.SOLITON_KV_GATES === "1"
        ? [
            [
              4096,
              newEvidence,
              0,
              "evidence_identity_mismatch",
              "identity mismatch",
            ],
            [
              4096,
              newEvidence,
              512,
              "evidence_identity_mismatch",
              "identity mismatch",
            ],
            [
              4097,
              newEvidence,
              1024,
              "evidence_limit_exceeded",
              "workload exceeds qualified bounds",
            ],
          ]
        : [
            [
              4096,
              oldEvidence,
              undefined,
              "evidence_limit_exceeded",
              "workload exceeds qualified bounds",
            ],
            [
              4097,
              newEvidence,
              undefined,
              "evidence_limit_exceeded",
              "workload exceeds qualified bounds",
            ],
          ];
  for (const [
    context,
    path,
    kvCacheMiB,
    reason,
    logReason,
    allocatorCacheMiB,
  ] of trials) {
    const snapshot = await state();
    await act("settings", {
      settings: {
        ...snapshot.settings,
        vllm: {
          ...snapshot.settings.vllm,
          context,
          evidence: resolve(path),
          ...(kvCacheMiB === undefined ? {} : { kvCacheMiB }),
          ...(allocatorCacheMiB === undefined ? {} : { allocatorCacheMiB }),
        },
      },
    });
    await act("inspectVllm");
    const diagnostic = (await state()).vllmInspection;
    assert.equal(diagnostic.status, "blocked");
    assert.equal(diagnostic.reason, reason);
    await act("clearLogs");
    await act("start");
    await wait(async () => {
      assert.equal(
        forbidden().length,
        0,
        "refused profile spawned inference/counter",
      );
      const snapshot = await state();
      return (
        snapshot.status === "offline" &&
        snapshot.logs.some((line) => line.includes(logReason))
      );
    });
    await act("stop");
    assert.deepEqual((await state()).document, before.document);
    report.checks.push({
      context,
      diagnostic: reason,
      ...(kvCacheMiB === undefined ? {} : { kvCacheMiB }),
      ...(allocatorCacheMiB === undefined ? {} : { allocatorCacheMiB }),
      startupRefused: true,
      noInferenceOrCounter: true,
      historyPreserved: true,
    });
  }
  assert.equal(hash(await readFile(source)), hash(original));
  for (const [i, path] of [oldEvidence, newEvidence].entries())
    assert.equal(hash(await readFile(path)), report.evidenceSha256[i]);
  report.passed = true;
} finally {
  if (app && app.exitCode === null && app.signalCode === null) {
    if (headers) await act("stop").catch(() => {});
    const done = new Promise((r) => app.once("exit", r));
    app.kill("SIGINT");
    const timer = setTimeout(() => app.kill("SIGKILL"), 10000);
    await done;
    clearTimeout(timer);
  }
  await writeFile(output, JSON.stringify(report, null, 2) + "\n", {
    flag: "wx",
    mode: 0o600,
  });
  await rm(dir, { recursive: true, force: true });
}
console.log(JSON.stringify(report));
