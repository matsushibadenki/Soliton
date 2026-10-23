import test from "node:test";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import {
  benchmark,
  options,
  makePrompt,
  memorySummary,
  makeCases,
  startupFailure,
} from "./benchmark-inference.mjs";

test("bounds and loopback restriction", () => {
  assert.throws(() => options(["--url", "http://example.com"]));
  assert.throws(() => options(["--rows", "513"]));
  assert.throws(() => options(["--timeout", "0"]));
  assert.equal(makePrompt(256).match(/verification code is/g).length, 1);
});
async function fixture(
  t,
  {
    offline = true,
    fail = false,
    mock = false,
    delay = 0,
    lowMemory = false,
    engine = "strata",
    rejectRows = null,
    wrongAnswer = false,
    largeError = false,
  } = {},
) {
  const actions = [],
    bodies = [];
  let running = !offline;
  const server = createServer(async (req, res) => {
    let raw = "";
    for await (const chunk of req) raw += chunk;
    res.setHeader("Content-Type", "application/json");
    let data;
    if (req.url === "/studio/session") data = { token: "DO-NOT-EXPORT" };
    else if (req.headers["x-studio-token"] !== "DO-NOT-EXPORT") {
      res.statusCode = 401;
      data = {};
    } else if (req.url === "/studio/action") {
      const { action } = JSON.parse(raw);
      actions.push(action);
      running = action === "start";
      data = { ok: true };
    } else if (req.url === "/studio/state")
      data = {
        status: running ? "ready" : "offline",
        document: { generating: false, messages: [{ content: "PRIVATE" }] },
        preparation: { running: false },
        settings: { mock, engine, vllm: { context: 1024 } },
        memoryUsage: {
          current: lowMemory ? { availableBytes: 1024 } : null,
          engine: { current: null },
        },
        memoryTuning: {},
        architecture: "fixture",
        backend: "fixture",
      };
    else if (req.url === "/v1/models") data = { data: [{ id: "fixture" }] };
    else if (req.url === "/v1/chat/completions") {
      const body = JSON.parse(raw);
      bodies.push(body);
      if (delay) await new Promise((r) => setTimeout(r, delay));
      if (
        rejectRows !== null &&
        body.messages[0].content.includes(`Record ${rejectRows}:`)
      ) {
        res.statusCode = 400;
        data = {
          error: {
            code: largeError ? "PRIVATE-CODE" : "context_length_exceeded",
            message: "PRIVATE".repeat(largeError ? 3000 : 1),
          },
        };
      } else if (fail) {
        res.statusCode = 500;
        data = { error: "PRIVATE" };
      } else
        data = {
          choices: [
            {
              message: {
                content: wrongAnswer
                  ? "PRIVATE-WRONG-ANSWER"
                  : body.messages.length === 3
                    ? "SOLITON-4826"
                    : "STRATA-7391",
              },
              finish_reason: "stop",
            },
          ],
          usage: { prompt_tokens: 41, completion_tokens: 8 },
          timings: { predicted_per_second: 1.2 },
        };
    } else {
      res.statusCode = 404;
      data = {};
    }
    res.end(JSON.stringify(data));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  t.after(() => new Promise((r) => server.close(r)));
  return {
    actions,
    bodies,
    config: options([
      "--url",
      `http://127.0.0.1:${server.address().port}`,
      "--rows",
      "0",
      "--start",
    ]),
  };
}
test("owned engine is stopped; reported counts retained; no private content exported", async (t) => {
  const f = await fixture(t);
  const r = await benchmark(f.config, () => {});
  assert.equal(r.error, undefined);
  assert.deepEqual(f.actions, ["start", "stop"]);
  assert.equal(r.cases[0].usage.prompt_tokens, 41);
  assert.equal(r.cases[0].retrieval, "exact");
  assert.equal(f.bodies[0].chat_template_kwargs.enable_thinking, false);
  assert(!JSON.stringify(r).includes("PRIVATE"));
  assert(!JSON.stringify(r).includes("DO-NOT-EXPORT"));
});
test("failure cleans up owned engine and does not export upstream error bodies", async (t) => {
  const f = await fixture(t, { fail: true });
  const r = await benchmark(f.config, () => {});
  assert.deepEqual(f.actions, ["start", "stop"]);
  assert.equal(r.cases[0].status, "failed");
  assert.equal(r.engineRestored, true);
  assert(!JSON.stringify(r).includes("PRIVATE"));
});
test("running engine stays running", async (t) => {
  const f = await fixture(t, { offline: false });
  const r = await benchmark(f.config, () => {});
  assert.equal(r.error, undefined);
  assert.deepEqual(f.actions, []);
});
test("mock engine is rejected before lifecycle changes", async (t) => {
  const f = await fixture(t, { mock: true });
  const r = await benchmark(f.config, () => {});
  assert.match(r.error, /Mock/);
  assert.deepEqual(f.actions, []);
});

test("memory summary retains missing readings and real zero values", () => {
  assert.equal(memorySummary([]).sampledEnginePeakBytes, null);
  const s = memorySummary([
    { system: { usedBytes: 20, availableBytes: 80 }, engine: { usedBytes: 0 } },
    {
      system: { usedBytes: 30, availableBytes: 70 },
      engine: { usedBytes: 12 },
    },
  ]);
  assert.equal(s.sampledEnginePeakBytes, 12);
  assert.equal(s.sampledMinimumAvailableBytes, 70);
});
test("timeout restores owned engine", async (t) => {
  const f = await fixture(t, { delay: 2000 });
  f.config.timeout = 1000;
  const r = await benchmark(f.config, () => {});
  assert.equal(r.cases[0].status, "timeout");
  assert.deepEqual(f.actions, ["start", "stop"]);
});
test("low available memory refuses startup", async (t) => {
  const f = await fixture(t, { lowMemory: true });
  const r = await benchmark(f.config, () => {});
  assert.match(r.error, /2 GiB/);
  assert.deepEqual(f.actions, []);
});

test("existing report is not overwritten and engine is not started", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "strata-benchmark-test-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const path = join(dir, "report.json");
  await writeFile(path, "keep");
  const f = await fixture(t);
  f.config.output = path;
  const r = await benchmark(f.config, () => {});
  assert.match(r.error, /EEXIST/);
  assert.deepEqual(f.actions, []);
  assert.equal(await readFile(path, "utf8"), "keep");
});

test("workload options are bounded and multilingual cases carry real message roles", () => {
  const config = options(["--suite", "workload"]);
  assert.deepEqual(config.rows, [0, 16, 32]);
  assert.deepEqual(config.languages, ["en", "ja", "zh"]);
  const cases = makeCases(config);
  assert.equal(cases.length, 12);
  assert.equal(cases.filter((c) => c.kind === "multi-turn").length, 3);
  assert.deepEqual(
    cases.at(-1).messages.map((m) => m.role),
    ["user", "assistant", "user"],
  );
  assert.throws(() => options(["--suite", "other"]));
  assert.throws(() => options(["--languages", "ja,ja"]));
  assert.throws(() => options(["--languages", "xx"]));
  assert.throws(() => options(["--languages"]));
});

test("workload measures three languages and multi-turn without exporting content", async (t) => {
  const f = await fixture(t);
  Object.assign(f.config, {
    suite: "workload",
    rows: [0, 16],
    languages: ["en", "ja", "zh"],
  });
  const r = await benchmark(f.config, () => {});
  assert.equal(r.plannedCases, 9);
  assert.equal(r.coverage.exact, 9);
  assert.equal(r.passed, true);
  assert.equal(r.cases.filter((c) => c.turns === 3).length, 3);
  assert.deepEqual(f.actions, ["start", "stop"]);
  assert(!JSON.stringify(r).includes("STRATA-7391"));
  assert(!JSON.stringify(r).includes("SOLITON-4826"));
  assert(!JSON.stringify(r).includes("PRIVATE"));
});

test("context refusal is missing coverage, independent shorter cases still run", async (t) => {
  const f = await fixture(t, { rejectRows: 64 });
  Object.assign(f.config, {
    suite: "workload",
    rows: [0, 64],
    languages: ["en", "ja", "zh"],
  });
  const r = await benchmark(f.config, () => {});
  assert.equal(r.error, undefined);
  assert.equal(r.passed, false);
  assert.equal(r.coverage.rejected, 3);
  assert.equal(r.coverage.exact, 6);
  assert.equal(r.coverage.incorrect, 0);
  assert(
    r.cases
      .filter((c) => c.status === "rejected")
      .every(
        (c) =>
          c.rejectionCode === "context_length_exceeded" && c.httpStatus === 400,
      ),
  );
  assert.deepEqual(f.actions, ["start", "stop"]);
  assert(!JSON.stringify(r).includes("PRIVATE"));
});

test("oversized private error bodies and unknown error codes never leak into reports", async (t) => {
  const f = await fixture(t, { rejectRows: 64, largeError: true });
  Object.assign(f.config, { suite: "workload", rows: [64], languages: ["ja"] });
  const r = await benchmark(f.config, () => {});
  assert.equal(r.coverage.rejected, 1);
  assert.equal(r.cases[0].rejectionCode, null);
  assert(!JSON.stringify(r).includes("PRIVATE"));
});

test("vLLM selection ignores the retained Strata-only mock preference", async (t) => {
  const f = await fixture(t, { engine: "vllmApple", mock: true });
  const r = await benchmark(f.config, () => {});
  assert.equal(r.passed, true);
  assert.equal(r.environment.selectedEngine, "vllmApple");
  assert.equal(r.environment.context, 1024);
});

test("completed but incorrect output does not pass and is not exported", async (t) => {
  const f = await fixture(t, { wrongAnswer: true });
  const r = await benchmark(f.config, () => {});
  assert.equal(r.cases[0].status, "completed");
  assert.equal(r.coverage.incorrect, 1);
  assert.equal(r.passed, false);
  assert(!JSON.stringify(r).includes("PRIVATE-WRONG-ANSWER"));
});

test("startup failures expose stable evidence reasons without process logs", () => {
  assert.equal(
    startupFailure([
      "PRIVATE architecture evidence identity mismatch /private/model",
    ]),
    "evidence_identity_mismatch",
  );
  assert.equal(
    startupFailure(["architecture evidence expired"]),
    "evidence_expired",
  );
  assert.equal(
    startupFailure(["private unknown failure"]),
    "engine_startup_failed",
  );
});
