import test from "node:test";
import assert from "node:assert/strict";
import { cacheHistory, verifyCacheMemory } from "./cache-memory-workload.mjs";

test("diverse prefixes retain six historical pairs and keep the identifier out of the last question", () => {
  const prefixes = new Set();
  for (let n = 0; n < 16; n++) {
    const { messages } = cacheHistory(n);
    assert.equal(messages.length, 13);
    assert(!messages.at(-1).content.includes("STRATA-7391"));
    prefixes.add(messages[0].content.split("\n")[0]);
  }
  assert.equal(prefixes.size, 16);
});

function mock(t, { missing = false, capacity = 10 } = {}) {
  const original = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = original;
  });
  const lru = new Map();
  globalThis.fetch = async (_url, init) => {
    const body = JSON.parse(init.body);
    if (4000 + body.max_tokens > 4096)
      return Response.json(
        { error: { code: "context_length_exceeded" } },
        { status: 422 },
      );
    const id = body.messages[0].content.split("\n")[0];
    const cached = lru.has(id) ? 3999 : 0;
    lru.delete(id);
    lru.set(id, true);
    while (lru.size > capacity) lru.delete(lru.keys().next().value);
    return Response.json({
      choices: [{ message: { content: "STRATA-7391" } }],
      usage: {
        prompt_tokens: 4000,
        ...(missing
          ? {}
          : { prompt_tokens_details: { cached_tokens: cached } }),
      },
    });
  };
}
const run = () =>
  verifyCacheMemory({
    base: "http://127.0.0.1:1",
    context: 4096,
    sampleMemory: async () => ({
      enginePhysicalFootprint: { usedBytes: 7 * 1024 ** 3 },
      systemAvailableBytes: 8 * 1024 ** 3,
    }),
  });

test("reported cold/hot/evicted/reheated counts validate two independent churn cycles and exact boundary", async (t) => {
  mock(t);
  const report = await run();
  assert.equal(report.cases.length, 38);
  assert.equal(report.samples.length, 42);
  assert.equal(report.observedAnchorReuseLosses, 2);
  assert.equal(report.boundary.rejectedTotal, 4097);
});
test("missing reuse accounting cannot be classified as eviction", async (t) => {
  mock(t, { missing: true });
  await assert.rejects(run(), /cache accounting unavailable/);
});
test("a cache large enough to retain the anchor cannot falsely pass eviction verification", async (t) => {
  mock(t, { capacity: 32 });
  await assert.rejects(run(), /remained fully reused/);
});

test("bounded retention may evict the recent history but must preserve answers, reheat and seeded accounting", async (t) => {
  mock(t, { capacity: 1 });
  const report = await verifyCacheMemory({
    base: "http://127.0.0.1:1",
    context: 4096,
    recentReuseRequired: false,
    verifySeeded: true,
    sampleMemory: async () => ({
      enginePhysicalFootprint: { usedBytes: 3 * 1024 ** 3 },
      systemAvailableBytes: 8 * 1024 ** 3,
      kvCache: { retained_bytes: 800 * 1024 ** 2, max_bytes: 1024 ** 3 },
    }),
  });
  assert.equal(report.cases.length, 38);
  assert.equal(report.seededCases.length, 3);
  assert(
    report.cases
      .filter((c) => c.phase.startsWith("recent"))
      .every((c) => c.cachedTokens === 0),
  );
});
