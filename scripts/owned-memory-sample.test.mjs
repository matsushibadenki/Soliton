import test from "node:test";
import assert from "node:assert/strict";
import { stableOwnedRss } from "./owned-memory-sample.mjs";
test("a disappearing transient child is resampled without zero substitution", async () => {
  let calls = 0;
  const result = await stableOwnedRss({
    descendants: () => (++calls === 1 ? [1, 2, 3] : [1, 2]),
    readRows: () => [
      [1, 10],
      [2, 20],
    ],
    sleep: async () => {},
  });
  assert.deepEqual(result, {
    ownedProcessCount: 2,
    ownedRssKiB: 30,
    sampleRetries: 1,
  });
});
test("persistent missing processes fail instead of making the sample smaller", async () => {
  await assert.rejects(
    stableOwnedRss({
      descendants: () => [1, 2],
      readRows: () => [[1, 10]],
      sleep: async () => {},
    }),
    /could not stabilize/,
  );
});
test("an absent tree is never a zero-memory sample", async () => {
  await assert.rejects(
    stableOwnedRss({
      descendants: () => [],
      readRows: () => [],
      sleep: async () => {},
    }),
    /tree disappeared/,
  );
});
