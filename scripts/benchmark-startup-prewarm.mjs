// Qualified headless first-request comparison; no UI/state polling after Start.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { readFile, writeFile, open, mkdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";
const args = process.argv.slice(2),
  value = (n) => args[args.indexOf(n) + 1];
assert(args.includes("--isolated"));
for (const name of ["--before", "--after", "--settings", "--output-dir"])
  assert(args.includes(name));
const dir = resolve(value("--output-dir"));
await mkdir(dir, { mode: 0o700 });
const settings = JSON.parse(await readFile(value("--settings"))).settings;
assert.equal(settings.engine, "vllmApple");
assert.equal(settings.vllm.backend, "mlx_lm");
assert.equal(settings.vllm.context, 4096);
const binaries = {
  before: resolve(value("--before")),
  after: resolve(value("--after")),
};
const report = {
  result: "running",
  scope:
    "No state polling after Start; six-second idle before first request. Normal desktop load; initialized work is moved earlier, not eliminated. No GPU-kernel speedup claim.",
  trials: [],
  binarySha256: {},
};
for (const [key, path] of Object.entries(binaries))
  report.binarySha256[key] = createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
report.evidenceSha256 = createHash("sha256")
  .update(await readFile(settings.vllm.evidence))
  .digest("hex");
const save = () =>
  writeFile(join(dir, "report.json"), JSON.stringify(report, null, 2) + "\n", {
    mode: 0o600,
  });
const processes = () =>
  execFileSync("/bin/ps", ["-axo", "pid=,ppid=,command="], {
    encoding: "utf8",
    maxBuffer: 8 * 1024 ** 2,
  })
    .split("\n")
    .map((line) => line.match(/^\s*(\d+)\s+(\d+)\s+(.*)$/))
    .filter(Boolean)
    .map((m) => ({ pid: +m[1], parent: +m[2], command: m[3] }));
function waitExit(child) {
  return new Promise((resolve, reject) => {
    if (child.exitCode !== null) return resolve();
    const timer = setTimeout(() => reject(Error("Host did not stop")), 15000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}
try {
  for (const [index, mode] of [
    "before",
    "after",
    "after",
    "before",
  ].entries()) {
    const api = "http://127.0.0.1:18098",
      statePath = join(dir, `${index}.state.json`);
    await writeFile(statePath, JSON.stringify({ settings, document: {} }), {
      mode: 0o600,
    });
    const log = await open(join(dir, `${index}.log`), "wx", 0o600);
    const host = spawn(binaries[mode], [], {
      cwd: resolve("."),
      env: {
        ...process.env,
        STUDIO_STATE_PATH: statePath,
        STUDIO_PORT: "18098",
      },
      stdio: ["ignore", log.fd, log.fd],
    });
    let headers,
      owned = false;
    const row = { mode, index };
    report.trials.push(row);
    async function request(path, body) {
      const response = await fetch(api + path, {
        method: body === undefined ? "GET" : "POST",
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(30000),
      });
      assert(response.ok, `HTTP ${response.status} at ${path}`);
      return response;
    }
    const action = (action) =>
      request("/studio/action", { action }).then((r) => r.json());
    try {
      // The bind-success line comes from this child, so an unrelated listener
      // cannot be mistaken for our QA host if its port was already occupied.
      let listening = false;
      for (let i = 0; i < 100; i++) {
        assert.equal(host.exitCode, null, "QA host exited before binding");
        const output = await readFile(join(dir, `${index}.log`), "utf8");
        if (output.includes(`Soliton: ${api}`)) {
          listening = true;
          break;
        }
        await new Promise((r) => setTimeout(r, 50));
      }
      assert(listening, "Owned QA host did not bind");
      const { token } = await fetch(api + "/studio/session", {
        method: "POST",
        headers: { Origin: api },
      }).then((r) => r.json());
      headers = { "Content-Type": "application/json", "x-studio-token": token };
      const initial = await request("/studio/state").then((r) => r.json());
      assert.equal(initial.status, "offline");
      owned = true;
      await action("start");
      await new Promise((r) => setTimeout(r, 6000));
      const counters = processes().filter(
        (p) =>
          p.parent === host.pid &&
          p.command.includes("--soliton-token-counter"),
      );
      row.counterBeforeFirstRequest = counters.length;
      assert.equal(counters.length, mode === "after" ? 1 : 0);
      const body = {
        model: "default_model",
        messages: [
          {
            role: "user",
            content:
              "The fictional code word is soliton. ".repeat(20) +
              "What is the code word? Answer briefly.",
          },
        ],
        max_tokens: 16,
        temperature: 0,
        stream: true,
        stream_options: { include_usage: true },
      };
      row.metrics = [];
      let answer;
      for (let repeat = 0; repeat < 2; repeat++) {
        const raw = await request("/v1/chat/completions", body).then((r) =>
          r.text(),
        );
        assert(raw.includes("[DONE]"));
        let text = "";
        for (const line of raw.split("\n")) {
          if (!line.startsWith("data:") || line.includes("[DONE]")) continue;
          const frame = JSON.parse(line.slice(5));
          text += frame.choices?.[0]?.delta?.content || "";
        }
        assert(text.toLowerCase().includes("soliton"));
        if (answer !== undefined) assert.equal(answer, text);
        answer = text;
        let snapshot, metrics;
        for (let i = 0; i < 100; i++) {
          snapshot = await request("/studio/state").then((r) => r.json());
          metrics = snapshot.requests.find(
            (r) => !row.metrics.some((m) => m.id === r.id),
          );
          if (metrics) break;
          await new Promise((r) => setTimeout(r, 20));
        }
        assert(metrics && metrics.status === "completed");
        row.metrics.push(metrics);
        row.startup = snapshot.startup;
      }
      row.answer = answer;
      await action("stop");
      owned = false;
      for (const p of counters)
        assert(
          !processes().some(
            (current) => current.pid === p.pid && current.parent === host.pid,
          ),
        );
      row.passed = true;
      console.log(
        JSON.stringify({
          mode,
          prepared: row.counterBeforeFirstRequest,
          admissionMs: row.metrics[0].gateway.admissionMs,
          firstContentMs: row.metrics[0].gateway.firstContentMs,
        }),
      );
    } finally {
      try {
        if (owned && headers) await action("stop");
      } finally {
        host.kill("SIGINT");
        try {
          await waitExit(host);
        } catch (error) {
          host.kill("SIGKILL");
          throw error;
        }
        await log.close();
        await save();
      }
    }
  }
  assert.equal(new Set(report.trials.map((r) => r.answer)).size, 1);
  for (const repeat of [0, 1]) {
    for (const field of ["prompt", "completion", "cached"]) {
      assert.equal(
        new Set(report.trials.map((r) => r.metrics[repeat].usage[field])).size,
        1,
        `${field} differs across starts at repeat ${repeat}`,
      );
    }
    assert.equal(
      new Set(report.trials.map((r) => r.metrics[repeat].finishReason)).size,
      1,
    );
  }
  const median = (values) => {
    const sorted = values.toSorted((a, b) => a - b);
    return (sorted[0] + sorted[1]) / 2;
  };
  report.summary = Object.fromEntries(
    ["before", "after"].map((mode) => {
      const rows = report.trials.filter((r) => r.mode === mode);
      return [
        mode,
        {
          firstAdmissionMedianMs: median(
            rows.map((r) => r.metrics[0].gateway.admissionMs),
          ),
          firstContentMedianMs: median(
            rows.map((r) => r.metrics[0].gateway.firstContentMs),
          ),
          repeatAdmissionMedianMs: median(
            rows.map((r) => r.metrics[1].gateway.admissionMs),
          ),
        },
      ];
    }),
  );
  report.result = "passed";
  await save();
  console.log(JSON.stringify(report.summary));
} catch (error) {
  report.result = "failed";
  report.error = String(error);
  await save();
  throw error;
}
