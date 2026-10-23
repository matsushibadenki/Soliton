import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
const [beforePath, afterPath, output, mode = "reuse"] = process.argv.slice(2);
assert(["reuse", "footprint", "stream", "prewarm"].includes(mode));
assert(
  beforePath && afterPath && output,
  "node scripts/compare-token-count.mjs BEFORE AFTER NEW_REPORT",
);
const streaming = mode === "stream" || mode === "prewarm";
const before = JSON.parse(await readFile(beforePath));
const after = JSON.parse(await readFile(afterPath));
assert.equal(before.passed, true);
assert.equal(after.passed, true);
assert.equal(before.evidenceSha256, after.evidenceSha256);
assert.equal(before.settingsFingerprint, after.settingsFingerprint);
assert.equal(before.context, after.context);
assert.equal(before.cases.length, 30);
assert.equal(after.cases.length, 30);
for (let i = 0; i < before.cases.length; i++) {
  const a = before.cases[i],
    b = after.cases[i];
  for (const k of [
    "round",
    "language",
    "kind",
    "rows",
    "status",
    "exact",
    "rejectionCode",
  ])
    assert.equal(a[k], b[k], `case ${i}: ${k}`);
  if (a.exact) {
    for (const k of ["prompt_tokens", "completion_tokens", "total_tokens"])
      assert.equal(a.usage[k], b.usage[k], `case ${i}: ${k}`);
    assert(b.usage.prompt_tokens + 64 <= after.context);
  }
}
assert.equal(before.counterProcessesAfterRequests, mode === "reuse" ? 0 : 1);
assert.equal(after.counterProcessesAfterRequests, 1);
assert.equal(after.checks.stopReapsWholeTree, true);
assert.equal(after.checks.noCounterWithStrata, true);
assert(
  after.checks.nativeSignalReapsTree || after.checks.headlessSignalReapsTree,
);
if (streaming) {
  assert(before.coldStream.exact && after.coldStream.exact);
  assert(
    before.checks.exactContextBoundary && after.checks.exactContextBoundary,
  );
  assert(after.coldStream.firstContentMs < before.coldStream.firstContentMs);
}
if (mode === "prewarm") {
  assert.equal(
    after.checks.counterOverlapsModelLoadingAfterQualification,
    true,
  );
  for (const report of [before, after]) {
    const early = report.startupObservations.filter(
      (s) => s.phase === "starting/false",
    );
    assert(early.length > 0);
    assert(early.every((s) => s.counterProcesses === 0));
  }
  assert(
    after.startup.counterSpawnedMs >= after.startup.controlReadyObservedMs,
  );
  assert(
    after.startup.counterSpawnedMs < after.startup.inferenceReadyObservedMs,
  );
}
const b = before.summary.warmMedianMs,
  a = after.summary.warmMedianMs;
if (mode === "reuse") assert(a < b, "warm median did not improve");
else {
  if (mode === "footprint")
    assert(
      after.summary.firstAcceptedMs < before.summary.firstAcceptedMs,
      "first response did not improve",
    );
  assert.equal(before.counterRssKiB.length, 1);
  assert.equal(after.counterRssKiB.length, 1);
  if (mode !== "prewarm")
    assert(
      after.counterRssKiB[0] < before.counterRssKiB[0],
      "counter RSS did not improve",
    );
  // Warm HTTP timing includes GPU inference and desktop contention; report it
  // independently of the first-response/RSS acceptance criteria.
}
const report = {
  schema: 1,
  mode,
  passed: true,
  hardware: "Apple M4 / 32 GiB",
  scope:
    "Gemma 2 2B 4-bit, context 1024, concurrency 1; EN/JA/ZH retrieval and synthetic multi-turn; 2 rounds",
  before: beforePath,
  after: afterPath,
  evidenceSha256: after.evidenceSha256,
  countsAndDecisionsIdentical: 30,
  acceptedExactPerRun: 24,
  rejectedOverLimitPerRun: 6,
  firstResponseBasis: streaming
    ? "First nonempty stream content after engine readiness, shared Rust headless core"
    : "First complete HTTP response after engine readiness, native Tauri",
  firstResponseReductionPercent: Number(
    (
      100 *
      (1 -
        (streaming
          ? after.coldStream.firstContentMs / before.coldStream.firstContentMs
          : after.summary.firstAcceptedMs / before.summary.firstAcceptedMs))
    ).toFixed(2),
  ),
  beforeCounterRssMiB: before.counterRssKiB.map((k) =>
    Number((k / 1024).toFixed(2)),
  ),
  counterRssReductionPercent:
    mode !== "reuse"
      ? Number(
          (
            100 *
            (1 - after.counterRssKiB[0] / before.counterRssKiB[0])
          ).toFixed(2),
        )
      : null,
  beforeStartup: before.startup,
  afterStartup: after.startup,
  beforeColdStream: before.coldStream,
  afterColdStream: after.coldStream,
  firstContentReductionPercent: streaming
    ? Number(
        (
          100 *
          (1 -
            after.coldStream.firstContentMs / before.coldStream.firstContentMs)
        ).toFixed(2),
      )
    : null,
  beforeSummary: before.summary,
  afterSummary: after.summary,
  warmMedianReductionPercent: Number((100 * (1 - a / b)).toFixed(2)),
  warmMedianSpeedup: Number((b / a).toFixed(2)),
  retainedCounterRssMiB: after.counterRssKiB.map((k) =>
    Number((k / 1024).toFixed(2)),
  ),
  checks: after.checks,
  limitations: [
    mode !== "reuse"
      ? streaming
        ? "Shared Rust headless debug builds, one first-content observation per version; OS caches retained, startup qualification excluded. See alternating CPU-only trials for repeated samples."
        : "Native first response is one fresh worker observation per version, with OS caches retained; see alternating CPU-only cold trials for repeated startup samples."
      : "First request still imports/loads tokenizer; warm reuse removes repeated initialization.",
    "HTTP elapsed includes counting, inference and transport; normal desktop activity, not an isolated GPU performance certification.",
    "Counter retains tokenizer metadata only during the selected engine session; no request/answer/count cache or model weights.",
    "Switch-to-Strata check uses the mock, not a real Strata model.",
  ],
};
await writeFile(output, JSON.stringify(report, null, 2) + "\n", {
  flag: "wx",
  mode: 0o600,
});
console.log(JSON.stringify(report, null, 2));
