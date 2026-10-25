// Diverse long histories, exact answers, backend-reported reuse and native memory.
import assert from "node:assert/strict";
import { contextMessages } from "./context-matrix.mjs";

export function cacheHistory(index) {
  assert(Number.isInteger(index) && index >= 0 && index < 16);
  const language = ["en", "ja", "zh"][index % 3];
  const messages = contextMessages(232, language, true, "middle");
  messages[0].content =
    `Batch ${String(index).padStart(3, "0")}.\n` + messages[0].content;
  return { language, messages };
}

export async function verifyCacheMemory({
  base,
  headers,
  context,
  sampleMemory,
  recentReuseRequired = true,
  verifySeeded = false,
}) {
  assert.equal(context, 4096);
  const started = performance.now(),
    cases = [],
    samples = [];
  const request = async (index, phase) => {
    const { language, messages } = cacheHistory(index);
    const start = performance.now();
    const response = await fetch(base + "/v1/chat/completions", {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: "default_model",
        messages,
        max_tokens: 64,
        temperature: 0,
        stream: false,
      }),
      signal: AbortSignal.timeout(180000),
    });
    assert.equal(
      response.status,
      200,
      `cache workload ${phase}/${index} HTTP ${response.status}`,
    );
    const body = await response.json();
    assert.equal(body.choices[0].message.content.trim(), "STRATA-7391");
    const prompt = body.usage?.prompt_tokens;
    const cached = body.usage?.prompt_tokens_details?.cached_tokens;
    assert(
      Number.isSafeInteger(prompt) && prompt >= 3500 && prompt + 64 <= context,
    );
    assert(
      Number.isSafeInteger(cached) && cached >= 0 && cached <= prompt,
      "backend cache accounting unavailable",
    );
    const previous = cases.find((c) => c.index === index);
    if (previous) assert.equal(prompt, previous.promptTokens);
    cases.push({
      phase,
      index,
      language,
      messageCount: messages.length,
      promptTokens: prompt,
      cachedTokens: cached,
      exact: true,
      elapsedMs: performance.now() - start,
    });
    const memory = await sampleMemory();
    assert(
      memory?.enginePhysicalFootprint?.usedBytes > 0,
      "native physical footprint unavailable",
    );
    assert(
      memory.systemAvailableBytes > 1024 ** 3,
      "insufficient available system memory",
    );
    samples.push({ phase, index, ...memory });
    console.error(
      `cache workload: ${phase}/${index} cached ${cached}/${prompt}`,
    );
    return cases.at(-1);
  };
  samples.push({ phase: "baseline", ...(await sampleMemory()) });
  const cold = await request(0, "anchor-cold");
  const hot = await request(0, "anchor-hot");
  assert(
    hot.cachedTokens > cold.cachedTokens &&
      hot.cachedTokens >= hot.promptTokens - 64,
  );
  const reserve = context - hot.promptTokens;
  for (const extra of [0, 1]) {
    const { messages } = cacheHistory(0);
    const response = await fetch(base + "/v1/chat/completions", {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: "default_model",
        messages,
        max_tokens: reserve + extra,
        temperature: 0,
      }),
      signal: AbortSignal.timeout(180000),
    });
    const body = await response.json();
    assert.equal(response.status, extra ? 422 : 200);
    if (extra) assert.equal(body.error.code, "context_length_exceeded");
    else {
      assert.equal(body.usage.prompt_tokens, hot.promptTokens);
      assert.equal(body.choices[0].message.content.trim(), "STRATA-7391");
    }
  }
  for (let round = 0; round < 2; round++) {
    for (let index = 1; index < 16; index++)
      await request(index, `churn-${round}`);
    const evicted = await request(0, `revisit-${round}`);
    assert(
      evicted.cachedTokens < hot.cachedTokens / 2,
      "anchor remained fully reused after churn",
    );
    const reheated = await request(0, `reheat-${round}`);
    assert(reheated.cachedTokens >= reheated.promptTokens - 64);
    const recent = await request(15, `recent-${round}`);
    if (recentReuseRequired)
      assert(recent.cachedTokens >= recent.promptTokens - 64);
  }
  for (const seconds of [0, 5, 10]) {
    if (seconds) await new Promise((resolve) => setTimeout(resolve, 5000));
    samples.push({
      phase: "idle",
      idleSeconds: seconds,
      ...(await sampleMemory()),
    });
  }
  const seededCases = [];
  if (verifySeeded) {
    for (const index of [1, 2, 3]) {
      const { messages } = cacheHistory(index);
      const response = await fetch(base + "/v1/chat/completions", {
        method: "POST",
        headers,
        body: JSON.stringify({
          model: "default_model",
          messages,
          max_tokens: 64,
          temperature: 0,
          seed: 42,
        }),
        signal: AbortSignal.timeout(180000),
      });
      assert.equal(response.status, 200);
      const body = await response.json();
      assert.equal(body.choices[0].message.content.trim(), "STRATA-7391");
      assert.equal(
        body.usage.prompt_tokens,
        cases.find((c) => c.index === index).promptTokens,
      );
      const memory = await sampleMemory();
      assert(
        memory.kvCache &&
          memory.kvCache.retained_bytes <= memory.kvCache.max_bytes,
      );
      seededCases.push({
        index,
        seed: 42,
        exact: true,
        promptTokens: body.usage.prompt_tokens,
        memory,
      });
    }
  }
  return {
    passed: true,
    elapsedMs: performance.now() - started,
    cases,
    samples,
    recentReuseRequired,
    seededCases,
    observedAnchorReuseLosses: 2,
    boundary: {
      passed: true,
      acceptedTotal: context,
      rejectedTotal: context + 1,
    },
    observations: verifySeeded
      ? "backend-reported reused tokens and sampled LRU logical bytes/allocator metrics; not unique physical KV ownership or forced OS pressure"
      : "backend-reported reused token counts; anchor misses after churn are consistent with LRU eviction, not direct inventory proof; no direct KV-byte accounting or forced OS pressure",
  };
}
