// Exact usage-driven boundary checks through the shared Rust admission path.
import assert from "node:assert/strict";
import { makePrompt } from "./benchmark-inference.mjs";

export function contextMessages(rows, language, history, position) {
  assert(Number.isInteger(rows) && rows >= 0 && rows <= 512);
  assert(["en", "ja", "zh"].includes(language));
  assert(["start", "middle", "end"].includes(position));
  const lines = makePrompt(rows, language).split("\n");
  const question = lines.pop();
  const [needle] = lines.splice(Math.floor(rows / 2), 1);
  lines.splice(
    position === "start" ? 0 : position === "end" ? rows : Math.floor(rows / 2),
    0,
    needle,
  );
  if (!history)
    return [{ role: "user", content: lines.join("\n") + "\n" + question }];
  const messages = [];
  const chunk = Math.ceil(lines.length / 6);
  for (let i = 0; i < lines.length; i += chunk) {
    messages.push({
      role: "user",
      content: lines.slice(i, i + chunk).join("\n"),
    });
    messages.push({ role: "assistant", content: "OK" });
  }
  messages.push({ role: "user", content: question });
  return messages;
}

export async function verifyContextMatrix({
  base,
  headers,
  context,
  sampleMemory = () => null,
}) {
  assert(Number.isInteger(context) && context >= 1024 && context <= 4096);
  const results = [],
    probes = [],
    memory = [];
  const positions = context > 1024 ? ["start", "middle", "end"] : ["middle"];
  const rounds = context > 1024 ? 3 : 1;
  const reservationCeiling = context > 1024 ? 128 : 511;
  const startedAt = new Date().toISOString(),
    started = performance.now();
  const request = async (messages, max_tokens, stream = false) =>
    fetch(base + "/v1/chat/completions", {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: "default_model",
        messages,
        max_tokens,
        stream,
        temperature: 0,
      }),
      signal: AbortSignal.timeout(180000),
    });
  for (const language of ["en", "ja", "zh"])
    for (const history of [false, true])
      for (const position of positions) {
        let low = 0,
          high = 512,
          rows = context === 1024 ? 54 : Math.floor((context - 160) / 16);
        let messages, prompt, reserve;
        for (let attempt = 0; attempt < 11; attempt++) {
          messages = contextMessages(rows, language, history, position);
          const probe = await request(messages, 32);
          const measured = await probe.json();
          probes.push({
            language,
            history,
            position,
            rows,
            status: probe.status,
          });
          if (probe.status === 422) {
            assert.equal(measured.error.code, "context_length_exceeded");
            high = rows - 1;
          } else {
            assert.equal(probe.status, 200);
            assert.equal(
              measured.choices[0].message.content.trim(),
              "STRATA-7391",
            );
            prompt = measured.usage.prompt_tokens;
            assert(Number.isSafeInteger(prompt) && prompt > 0);
            reserve = context - prompt;
            if (reserve >= 32 && reserve <= reservationCeiling) break;
            assert(reserve > reservationCeiling);
            low = rows + 1;
          }
          assert(
            low <= high,
            "no fixture fits the qualified output reservation",
          );
          rows = Math.floor((low + high) / 2);
          reserve = undefined;
        }
        assert(
          reserve >= 32 && reserve <= reservationCeiling,
          "fixture search exhausted",
        );
        for (let round = 0; round < rounds; round++)
          for (const stream of [false, true]) {
            const start = performance.now();
            const accepted = await request(messages, reserve, stream);
            assert.equal(accepted.status, 200);
            let answer = "";
            if (stream) {
              const decoder = new TextDecoder();
              let pending = "",
                done = false;
              for await (const chunk of accepted.body) {
                pending += decoder.decode(chunk, { stream: true });
                let end;
                while ((end = pending.indexOf("\n")) >= 0) {
                  const line = pending.slice(0, end).trim();
                  pending = pending.slice(end + 1);
                  if (line === "data: [DONE]") done = true;
                  else if (line.startsWith("data:")) {
                    const frame = JSON.parse(line.slice(5));
                    assert(!frame.error, "stream error frame");
                    answer += frame.choices?.[0]?.delta?.content || "";
                  }
                }
              }
              assert(done, "stream did not terminate normally");
            } else {
              const body = await accepted.json();
              assert.equal(body.usage.prompt_tokens, prompt);
              answer = body.choices[0].message.content;
            }
            assert.equal(answer.trim(), "STRATA-7391");
            const elapsedMs = performance.now() - start;
            const refused = await request(messages, reserve + 1, stream);
            assert.equal(refused.status, 422);
            assert.equal(
              (await refused.json()).error.code,
              "context_length_exceeded",
            );
            results.push({
              language,
              history,
              messageCount: messages.length,
              position,
              rows,
              round,
              stream,
              promptTokens: prompt,
              reservedOutputTokens: reserve,
              acceptedTotal: prompt + reserve,
              rejectedTotal: prompt + reserve + 1,
              exact: true,
              elapsedMs,
            });
            const sample = await sampleMemory();
            if (sample) memory.push(sample);
          }
        console.error(
          `context matrix: ${language}/history=${history}/${position} passed`,
        );
      }
  assert.equal(results.length, 3 * 2 * positions.length * rounds * 2);
  return {
    passed: true,
    startedAt,
    elapsedMs: performance.now() - started,
    rounds,
    reservationCeiling,
    probes,
    memory,
    cases: results,
  };
}
