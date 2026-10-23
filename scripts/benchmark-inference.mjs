// Local, opt-in inference measurements. Never downloads models or edits settings/history.
import { open } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";

export function options(args) {
  const result = {
    url: "http://127.0.0.1:8080",
    rows: [0, 64, 256],
    timeout: 240000,
    start: false,
    output: null,
    suite: "retrieval",
    languages: null,
  };
  for (let i = 0; i < args.length; i++) {
    const key = args[i];
    if (key === "--start") result.start = true;
    else if (key === "--url") result.url = args[++i];
    else if (key === "--rows") result.rows = args[++i]?.split(",").map(Number);
    else if (key === "--timeout") result.timeout = Number(args[++i]);
    else if (key === "--suite") result.suite = args[++i];
    else if (key === "--languages") {
      const value = args[++i];
      if (!value || value.startsWith("--"))
        throw Error("--languages needs en, ja, zh values");
      result.languages = value.split(",");
    } else if (key === "--output") result.output = args[++i];
    else throw Error(`Unknown option: ${key}`);
  }
  if (!["retrieval", "workload"].includes(result.suite))
    throw Error("Suite must be retrieval or workload");
  if (result.suite === "workload" && !args.includes("--rows"))
    result.rows = [0, 16, 32];
  result.languages ??=
    result.suite === "workload" ? ["en", "ja", "zh"] : ["en"];
  if (
    !result.languages.length ||
    result.languages.length > 3 ||
    new Set(result.languages).size !== result.languages.length ||
    result.languages.some((l) => !["en", "ja", "zh"].includes(l))
  )
    throw Error("Languages must be unique en, ja, zh values");
  const url = new URL(result.url);
  if (
    url.protocol !== "http:" ||
    url.hostname !== "127.0.0.1" ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    url.pathname !== "/"
  )
    throw Error("Use a plain loopback http://127.0.0.1:PORT URL");
  if (
    !result.rows?.length ||
    result.rows.length > 8 ||
    result.rows.some((n) => !Number.isInteger(n) || n < 0 || n > 512)
  )
    throw Error("Rows must contain 1–8 integers between 0 and 512");
  if (
    !Number.isInteger(result.timeout) ||
    result.timeout < 1000 ||
    result.timeout > 900000
  )
    throw Error("Timeout must be 1000–900000 ms");
  if (result.output === undefined) throw Error("--output needs a file path");
  result.url = url.origin;
  return result;
}
const prompts = {
  en: {
    code: "The verification code is STRATA-7391.",
    question: "Return only the verification code, with no explanation.",
    remember: "Remember this verification code: SOLITON-4826.",
    ack: "I will remember the code.",
    recall:
      "What verification code did I ask you to remember? Return only the code.",
  },
  ja: {
    code: "確認コードはSTRATA-7391です。",
    question: "確認コードだけを出力してください。説明は不要です。",
    remember: "確認コードSOLITON-4826を覚えてください。",
    ack: "確認コードを覚えました。",
    recall: "先ほど覚えるように伝えた確認コードだけを出力してください。",
  },
  zh: {
    code: "验证码是STRATA-7391。",
    question: "只输出验证码，不要解释。",
    remember: "请记住验证码SOLITON-4826。",
    ack: "我记住了验证码。",
    recall: "我刚才让你记住的验证码是什么？只输出验证码。",
  },
};
export function makePrompt(rows, language = "en") {
  const lines = Array.from(
    { length: rows },
    (_, i) =>
      `Record ${i + 1}: item ${String((i * 7919) % 10000).padStart(4, "0")} has status archived.`,
  );
  lines.splice(Math.floor(rows / 2), 0, prompts[language].code);
  return lines.join("\n") + "\n" + prompts[language].question;
}

export function makeCases(config) {
  const cases = [];
  for (const language of config.languages || ["en"]) {
    for (const rows of config.rows)
      cases.push({
        kind: "retrieval",
        language,
        rows,
        expected: "STRATA-7391",
        messages: [{ role: "user", content: makePrompt(rows, language) }],
      });
    if (config.suite === "workload") {
      const p = prompts[language];
      cases.push({
        kind: "multi-turn",
        language,
        rows: null,
        expected: "SOLITON-4826",
        messages: [
          { role: "user", content: p.remember },
          { role: "assistant", content: p.ack },
          { role: "user", content: p.recall },
        ],
      });
    }
  }
  return cases;
}
const safeCodes = new Set([
  "context_length_exceeded",
  "tokenization_unavailable",
  "token_budget_exceeded",
  "memory_pressure",
  "engine_busy",
  "runtime_maintenance",
  "invalid_request",
  "invalid_request_error",
  "invalid_request_token_budget",
  "unknown_model",
  "model_not_found",
]);
async function responseError(response, path) {
  // Error bodies may contain prompts/credentials; only bounded, allowlisted codes leave this function.
  let raw = "",
    bytes = 0;
  const reader = response.body?.getReader();
  try {
    if (reader)
      for (;;) {
        const part = await reader.read();
        if (part.done) break;
        bytes += part.value.byteLength;
        if (bytes > 8192) {
          raw = "";
          break;
        }
        raw += new TextDecoder().decode(part.value);
      }
  } finally {
    await reader?.cancel().catch(() => {});
  }
  let code;
  try {
    const v = JSON.parse(raw);
    code = v.error?.code || v.error?.type;
  } catch {}
  const error = new Error(`${path}: HTTP ${response.status}`);
  error.httpStatus = response.status;
  error.code = safeCodes.has(code) ? code : null;
  return error;
}
export function coverage(cases) {
  return Object.fromEntries(
    [
      "completed",
      "exact",
      "contains",
      "incorrect",
      "unanswered",
      "rejected",
      "failed",
      "timeout",
      "interrupted",
      "memory-limit",
    ].map((key) => [
      key,
      cases.filter((c) => c.status === key || c.retrieval === key).length,
    ]),
  );
}

export function memorySummary(samples) {
  const system = samples
    .map((s) => s.system?.usedBytes)
    .filter(Number.isFinite);
  const available = samples
    .map((s) => s.system?.availableBytes)
    .filter(Number.isFinite);
  const engine = samples
    .map((s) => s.engine?.usedBytes)
    .filter(Number.isFinite);
  return {
    sampledSystemPeakBytes: system.length ? Math.max(...system) : null,
    sampledEnginePeakBytes: engine.length ? Math.max(...engine) : null,
    sampledMinimumAvailableBytes: available.length
      ? Math.min(...available)
      : null,
    samples: samples.length,
  };
}

export function startupFailure(logs = []) {
  const text = logs.slice(-30).join("\n");
  for (const [pattern, code] of [
    [/architecture evidence identity mismatch/, "evidence_identity_mismatch"],
    [/architecture evidence expired/, "evidence_expired"],
    [
      /architecture evidence workload exceeds qualified bounds/,
      "evidence_limit_exceeded",
    ],
    [/architecture evidence report digest mismatch/, "evidence_corrupt"],
    [
      /incompatible .* environment|outside_verified_matrix/,
      "backend_incompatible",
    ],
    [
      /model_memory_hard_ceiling_exceeded|does not fit.*memory/,
      "model_memory_budget",
    ],
  ])
    if (pattern.test(text)) return code;
  return "engine_startup_failed";
}

export async function benchmark(config, emit = console.error) {
  const report = {
    schema: 1,
    startedAt: new Date().toISOString(),
    kind:
      config.suite === "workload"
        ? "multilingual-workload"
        : "synthetic-retrieval",
    plannedCases: makeCases(config).length,
    tokenCounts: "engine-reported only; rows are not tokens",
    samples: [],
    cases: [],
  };
  let headers,
    started = false,
    interrupted = false,
    active;
  const onInterrupt = () => {
    interrupted = true;
    active?.abort();
  };
  process.on("SIGINT", onInterrupt);
  process.on("SIGTERM", onInterrupt);
  async function api(path, init = {}, signal) {
    const response = await fetch(config.url + path, {
      ...init,
      headers: { ...headers, ...init.headers },
      signal: signal || AbortSignal.timeout(15000),
    });
    if (!response.ok) {
      throw await responseError(response, path);
    }
    return response.json();
  }
  const state = () => api("/studio/state");
  const action = (action) =>
    api("/studio/action", { method: "POST", body: JSON.stringify({ action }) });
  let poll, memoryLimit, outputFile;
  try {
    if (config.output) outputFile = await open(config.output, "wx", 0o600);
    const session = await api("/studio/session", {
      method: "POST",
      headers: { Origin: config.url },
    });
    headers = {
      "Content-Type": "application/json",
      "x-studio-token": session.token,
    };
    const initial = await state();
    if (
      initial.document.generating ||
      initial.preparation.running ||
      ["running", "cancelling"].includes(initial.preparation.status)
    )
      throw Error("Wait for the active generation/preparation to finish");
    if (initial.memoryUsage.current?.availableBytes < 2 * 1024 ** 3)
      throw Error("At least 2 GiB available memory is required to begin");
    if (initial.settings.mock && initial.settings.engine !== "vllmApple")
      throw Error("Mock engine is not a real inference benchmark");
    report.environment = {
      architecture: initial.architecture,
      selectedEngine: initial.settings.engine || "strata",
      context:
        initial.settings.engine === "vllmApple"
          ? (initial.settings.vllm?.context ?? null)
          : (initial.memoryTuning?.next?.context ?? null),
      backend: initial.backend,
      memoryBytes: initial.memoryBytes,
      memoryTuning:
        initial.settings.engine === "vllmApple" ? null : initial.memoryTuning,
      initialStatus: initial.status,
    };
    if (initial.status === "offline") {
      if (!config.start)
        throw Error(
          "Engine is offline. Start it in the app or pass --start (stopped again afterward)",
        );
      // Remember ownership before starting: failures or interruption still trigger cleanup.
      started = true;
      await action("start");
    }
    const readyDeadline = performance.now() + config.timeout;
    let ready;
    do {
      if (interrupted) throw Error("Interrupted");
      ready = await state();
      if (ready.status === "ready") break;
      if (performance.now() > readyDeadline || ready.status === "offline") {
        report.startupFailure = startupFailure(ready.logs);
        throw Error(`Engine did not become ready: ${report.startupFailure}`);
      }
      await new Promise((r) => setTimeout(r, 1000));
    } while (true);
    const models = await api("/v1/models");
    const model = models.data?.[0]?.id;
    if (!model) throw Error("Engine returned no model");
    report.model = model;
    const sample = async () => {
      const s = await state();
      report.samples.push({
        elapsedMs: Math.round(performance.now() - runStart),
        system: s.memoryUsage.current,
        engine: s.memoryUsage.engine.current,
      });
      if (s.memoryUsage.current?.availableBytes < 2 * 1024 ** 3) {
        memoryLimit =
          "Available system memory fell below 2 GiB; measurement stopped";
        active?.abort();
      }
    };
    const runStart = performance.now();
    await sample();
    // Sequential sampling avoids overlapping /studio/state requests on slow hardware.
    let sampling = true;
    poll = (async () => {
      while (sampling) {
        await new Promise((r) => setTimeout(r, 1000));
        if (sampling) await sample().catch(() => {});
      }
    })();
    try {
      for (const item of makeCases(config)) {
        if (interrupted) throw Error("Interrupted");
        if (memoryLimit) throw Error(memoryLimit);
        const begin = performance.now();
        const entry = {
          kind: item.kind,
          language: item.language,
          rows: item.rows,
          inputBytes: item.messages.reduce(
            (sum, m) => sum + Buffer.byteLength(m.content),
            0,
          ),
          turns: item.messages.length,
          startedMs: Math.round(begin - runStart),
        };
        report.cases.push(entry);
        active = new AbortController();
        const timeout = setTimeout(() => active?.abort(), config.timeout);
        emit(
          `Measuring ${item.language} ${item.kind}${item.rows == null ? "" : " / " + item.rows + " filler rows"}…`,
        );
        try {
          const result = await api(
            "/v1/chat/completions",
            {
              method: "POST",
              body: JSON.stringify({
                model,
                messages: item.messages,
                stream: false,
                temperature: 0,
                max_tokens: 64,
                chat_template_kwargs: { enable_thinking: false },
              }),
            },
            active.signal,
          );
          const choice = result.choices?.[0];
          if (!choice?.message || typeof choice.message.content !== "string")
            throw Error("Missing completion content");
          const answer = choice.message.content.trim();
          entry.httpStatus = 200;
          entry.status = "completed";
          entry.retrieval =
            answer === item.expected
              ? "exact"
              : answer.includes(item.expected)
                ? "contains"
                : answer
                  ? "incorrect"
                  : "unanswered";
          entry.finishReason = choice.finish_reason ?? null;
          entry.usage = result.usage ?? null;
          entry.timings = result.timings ?? null;
          // Store neither model text nor the user's messages/settings/session credentials.
        } catch (error) {
          entry.httpStatus = error.httpStatus ?? null;
          entry.rejectionCode = error.code ?? null;
          const rejected =
            [400, 413, 422].includes(error.httpStatus) ||
            (error.httpStatus === 503 && error.code === "memory_pressure");
          entry.status = memoryLimit
            ? "memory-limit"
            : interrupted
              ? "interrupted"
              : active.signal.aborted
                ? "timeout"
                : rejected
                  ? "rejected"
                  : "failed";
          entry.error = memoryLimit || error.message;
          // A rejected context is missing coverage, never an accuracy failure. Workload runs continue to shorter independent cases.
          if (!(config.suite === "workload" && entry.status === "rejected"))
            throw Error(entry.error);
        } finally {
          clearTimeout(timeout);
          active = null;
          entry.elapsedMs = Math.round(performance.now() - begin);
          await sample().catch(() => {});
          entry.endedMs = Math.round(performance.now() - runStart);
          entry.memory = memorySummary(
            report.samples.filter(
              (s) =>
                s.elapsedMs >= entry.startedMs && s.elapsedMs <= entry.endedMs,
            ),
          );
        }
      }
    } finally {
      sampling = false;
      await poll;
    }
  } catch (error) {
    report.error = error.message;
  } finally {
    if (started) {
      try {
        await action("stop");
        report.engineRestored = true;
      } catch {
        report.engineRestored = false;
        report.cleanupError = "Engine stop failed; check the app";
      }
    }
    process.off("SIGINT", onInterrupt);
    process.off("SIGTERM", onInterrupt);
    report.finishedAt = new Date().toISOString();
    report.memory = memorySummary(report.samples);
    report.coverage = coverage(report.cases);
    report.passed =
      !report.error &&
      !report.cleanupError &&
      report.cases.length === report.plannedCases &&
      report.cases.every(
        (c) => c.status === "completed" && c.retrieval === "exact",
      );
    if (outputFile) {
      try {
        await outputFile.writeFile(JSON.stringify(report, null, 2) + "\n");
      } finally {
        await outputFile.close();
      }
    }
  }
  return report;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  try {
    const report = await benchmark(options(process.argv.slice(2)));
    console.log(JSON.stringify(report, null, 2));
    if (!report.passed) process.exitCode = 1;
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
