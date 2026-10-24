import test from "node:test";
import assert from "node:assert/strict";
import { contextMessages, verifyContextMatrix } from "./context-matrix.mjs";

test("retrieval moves the needle and historical recall keeps it out of the latest message", () => {
  for (const language of ["en", "ja", "zh"]) {
    for (const position of ["start", "middle", "end"]) {
      const messages = contextMessages(20, language, true, position);
      assert.equal(messages.length, 13);
      assert(!messages.at(-1).content.includes("STRATA-7391"));
      const lines = messages
        .slice(0, -1)
        .filter((m) => m.role === "user")
        .flatMap((m) => m.content.split("\n"));
      assert.equal(
        lines.findIndex((line) => line.includes("STRATA-7391")),
        position === "start" ? 0 : position === "end" ? 20 : 10,
      );
    }
  }
});

function fixture(
  t,
  {
    context = 4096,
    truncated = false,
    wrongCount = false,
    wrongRefusal = false,
  } = {},
) {
  const original = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = original;
  });
  globalThis.fetch = async (_url, init) => {
    const body = JSON.parse(init.body);
    if (body.max_tokens === 101)
      return Response.json(
        {
          error: {
            code: wrongRefusal ? "engine_busy" : "context_length_exceeded",
          },
        },
        { status: 422 },
      );
    assert(
      [32, 100].includes(body.max_tokens),
      "reservation must come from actual reported usage",
    );
    if (!body.stream)
      return Response.json({
        choices: [{ message: { content: "STRATA-7391" } }],
        usage: {
          prompt_tokens:
            context - 100 + (wrongCount && body.max_tokens === 100 ? 1 : 0),
        },
      });
    const text =
      'data: {"choices":[{"delta":{"content":"STRATA-7391"}}]}\n\n' +
      (truncated ? "" : "data: [DONE]\n\n");
    const bytes = new TextEncoder().encode(text);
    return new Response(
      new ReadableStream({
        start(controller) {
          for (let i = 0; i < bytes.length; i += 3)
            controller.enqueue(bytes.slice(i, i + 3));
          controller.close();
        },
      }),
    );
  };
}

test("4096 matrix covers positions, history, streams and three repeated rounds with usage-derived boundaries", async (t) => {
  fixture(t);
  const result = await verifyContextMatrix({
    base: "http://127.0.0.1:1",
    context: 4096,
  });
  assert.equal(result.cases.length, 108);
  assert.equal(result.probes.length, 18);
  assert(
    result.cases.every(
      (row) => row.acceptedTotal === 4096 && row.rejectedTotal === 4097,
    ),
  );
});
for (const [name, flags] of Object.entries({
  truncatedStream: { truncated: true },
  changedUsage: { wrongCount: true },
  wrongRefusal: { wrongRefusal: true },
})) {
  test(`${name} cannot produce a passing matrix`, async (t) => {
    fixture(t, flags);
    await assert.rejects(
      verifyContextMatrix({ base: "http://127.0.0.1:1", context: 4096 }),
    );
  });
}

test("bounded fixture search recovers from an oversized first prompt using real response usage", async (t) => {
  const original = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = original;
  });
  globalThis.fetch = async (_url, init) => {
    const body = JSON.parse(init.body);
    const rows = body.messages.reduce(
      (sum, m) => sum + (m.content.match(/Record \d+:/g)?.length || 0),
      0,
    );
    const prompt = rows * 17 + 20;
    if (prompt + body.max_tokens > 4096)
      return Response.json(
        { error: { code: "context_length_exceeded" } },
        { status: 422 },
      );
    if (body.stream)
      return new Response(
        'data: {"choices":[{"delta":{"content":"STRATA-7391"}}]}\n\ndata: [DONE]\n\n',
      );
    return Response.json({
      choices: [{ message: { content: "STRATA-7391" } }],
      usage: { prompt_tokens: prompt },
    });
  };
  const result = await verifyContextMatrix({
    base: "http://127.0.0.1:1",
    context: 4096,
  });
  assert(result.probes.some((row) => row.status === 422));
  assert(result.probes.length > 18);
  assert(
    result.cases.every(
      (row) => row.promptTokens >= 3968 && row.promptTokens <= 4064,
    ),
  );
});
