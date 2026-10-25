// Resample short-lived descendant races; never replace a missing process with zero.
import assert from "node:assert/strict";
export async function stableOwnedRss({
  descendants,
  readRows,
  sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
}) {
  for (let attempt = 0; attempt < 5; attempt++) {
    const ids = descendants();
    assert(ids.length > 0, "owned process tree disappeared");
    let rows;
    try {
      rows = readRows(ids);
    } catch {
      rows = [];
    }
    const after = descendants();
    if (
      rows.length === ids.length &&
      after.length === ids.length &&
      after.every((id) => ids.includes(id)) &&
      rows.every(
        ([pid, rss]) => ids.includes(pid) && Number.isFinite(rss) && rss > 0,
      ) &&
      new Set(rows.map(([pid]) => pid)).size === ids.length
    )
      return {
        ownedProcessCount: rows.length,
        ownedRssKiB: rows.reduce((sum, [, rss]) => sum + rss, 0),
        sampleRetries: attempt,
      };
    if (attempt < 4) await sleep(50);
  }
  throw Error("owned memory sample could not stabilize");
}
