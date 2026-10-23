// Browser plugin not available: exercise the real Rust API via Playwright.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  writeFile,
  readFile,
  realpath,
  copyFile,
  rm,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { preferences, closePreferences, language } from "./ui-helpers.mjs";
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
});
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const fixture = await realpath(
  await mkdtemp(join(tmpdir(), "strata-reuse-ui-")),
);
let headers, baseline, baselinePreparation;
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
});
const state = () =>
  fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/studio/state",
    { headers },
  ).then((r) => r.json());
async function put(settings) {
  const r = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
      "/studio/action",
    {
      method: "POST",
      headers,
      body: JSON.stringify({ action: "settings", settings }),
    },
  );
  assert.equal(r.status, 200);
}
try {
  for (const dir of ["tools", "tokenizer", "models"])
    await mkdir(join(fixture, dir));
  await copyFile(
    resolve("Strata/tools/gguf_reader.py"),
    join(fixture, "tools/gguf_reader.py"),
  );
  const weights = join(fixture, "models/ready-fixture.gguf");
  const header = Buffer.alloc(24);
  header.write("GGUF");
  header.writeUInt32LE(3, 4);
  await writeFile(weights, header);
  await writeFile(join(fixture, "engine"), "fixture");
  await writeFile(join(fixture, "tokenizer/vocab.json"), "{}");
  const config = join(fixture, "strata-existing.json");
  await writeFile(
    config,
    JSON.stringify({
      exe: join(fixture, "engine"),
      args: ["--native", weights],
      tokenizer: join(fixture, "tokenizer"),
      backend: "metal",
      model_name: "Reuse fixture",
    }),
  );
  const incomplete = join(
    fixture,
    "models/Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf",
  );
  await writeFile(incomplete, header);
  await page.goto(process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180");
  const conn = await page.evaluate(() =>
    fetch("/studio/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    }).then((r) => r.json()),
  );
  headers = {
    "Content-Type": "application/json",
    "x-studio-token": conn.token,
  };
  const initial = await state();
  baseline = initial.settings;
  baselinePreparation = initial.preparation;
  await put({
    ...baseline,
    strataRoot: fixture,
    config: "",
    language: "en",
    modelDirectories: [],
  });
  await page.reload();
  await preferences(page, "engine");
  await page.getByRole("button", { name: "Local models", exact: true }).click();
  const dialog = page.locator(".model-library-dialog");
  await dialog.locator('.model-inventory[aria-busy="false"]').waitFor();
  await dialog
    .getByRole("button", {
      name: "Select model · Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf",
      exact: true,
    })
    .click();
  await dialog.getByText(/Missing model shards/).waitFor();
  assert(
    await dialog
      .getByRole("button", { name: "Use in Strata", exact: true })
      .isDisabled(),
  );
  assert.deepEqual((await state()).preparation, baselinePreparation);
  for (const width of [320, 375, 768, 1440]) {
    await page.setViewportSize({ width, height: 800 });
    assert(
      await dialog.evaluate(
        (e) =>
          e.getBoundingClientRect().left >= 0 &&
          e.getBoundingClientRect().right <= innerWidth,
      ),
    );
    assert(
      await dialog
        .locator(".model-use-footer")
        .evaluate((e) => e.scrollWidth <= e.clientWidth),
    );
  }
  await dialog
    .getByRole("button", {
      name: "Select model · ready-fixture.gguf",
      exact: true,
    })
    .click();
  await dialog
    .getByText("Existing Strata configuration", { exact: true })
    .waitFor();
  await page.screenshot({ path: "/tmp/strata-model-selected.png" });
  await dialog
    .getByRole("button", { name: "Use in Strata", exact: true })
    .click();
  await dialog.waitFor({ state: "hidden" });
  assert.equal((await state()).settings.config, config);
  assert.equal(
    await page
      .getByRole("textbox", { name: "Config path", exact: true })
      .inputValue(),
    config,
  );
  assert.deepEqual((await state()).preparation, baselinePreparation);
  assert.equal((await state()).settings.mock, false);
  // Complete a tiny supported-format fixture through the real supervised reuse job.
  function gguf(index) {
    const parts = [];
    const num = (n, bytes) => {
      const b = Buffer.alloc(bytes);
      bytes === 8 ? b.writeBigUInt64LE(BigInt(n)) : b.writeUInt32LE(n);
      parts.push(b);
    };
    const str = (s) => {
      num(Buffer.byteLength(s), 8);
      parts.push(Buffer.from(s));
    };
    parts.push(Buffer.from("GGUF"));
    num(3, 4);
    num(1, 8);
    num(4, 8);
    str("general.architecture");
    num(8, 4);
    str("qwen4exp");
    for (const [k, v] of [
      ["split.count", 2],
      ["split.no", index],
      ["split.tensors.count", 2],
    ]) {
      str(k);
      num(4, 4);
      num(v, 4);
    }
    str("tensor_" + index);
    num(1, 4);
    num(1, 8);
    num(0, 4);
    num(0, 8);
    const data = Buffer.concat(parts);
    return Buffer.concat([
      data,
      Buffer.alloc(Math.ceil(data.length / 32) * 32 - data.length + 4),
    ]);
  }
  const first = gguf(0);
  await writeFile(incomplete, first);
  await writeFile(
    join(fixture, "models/Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00002-of-00002.gguf"),
    gguf(1),
  );
  await mkdir(join(fixture, "metal"));
  await mkdir(join(fixture, "serve"));
  await writeFile(join(fixture, "serve/server.py"), "");
  await writeFile(
    join(fixture, "setup.py"),
    "def done(p): return False\ndef download(*a,**k): raise AssertionError('network forbidden')\n",
  );
  await writeFile(
    join(fixture, "metal/setup_mac.py"),
    `from pathlib import Path
import setup as S, json, time
ROOT=Path(__file__).resolve().parent.parent
def stream_mtp(*a): return True
def main(args):
    stage=Path(args[args.index('--gguf-dir')+1])
    files=sorted(stage.glob('*.gguf'))
    assert len(files)==2 and all(p.is_symlink() and S.done(p) for p in files)
    assert not stream_mtp(False,128)
    time.sleep(2)
    (ROOT/'strata-q2_0.json').write_text(json.dumps({'exe':str(ROOT/'engine'),'args':['--native',str(files[0])],'tokenizer':str(ROOT/'tokenizer'),'backend':'metal'}))
    return 0
`,
  );
  await page.getByRole("button", { name: "Local models", exact: true }).click();
  await dialog.locator('.model-inventory[aria-busy="false"]').waitFor();
  await dialog
    .getByRole("button", {
      name: "Select model · Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf",
      exact: true,
    })
    .click();
  await dialog.getByText(/Reuse files · no model download/).waitFor();
  await dialog
    .getByRole("button", { name: "Use in Strata", exact: true })
    .click();
  const setup = page.locator(".setup-dialog");
  await setup.waitFor();
  assert.equal(
    await setup.locator(".download-size code").textContent(),
    "0 GB",
  );
  assert.equal(await setup.locator(".download-consent").count(), 0);
  await setup
    .getByText("Model prepared. Configuration applied.", { exact: true })
    .waitFor({ timeout: 30000 });
  assert.equal(
    (await state()).settings.config,
    join(fixture, "strata-q2_0.json"),
  );
  assert.equal((await state()).status, "offline");
  assert.deepEqual(await readFile(incomplete), first);
  await page.keyboard.press("Escape");
  for (const [label, select, use] of [
    ["日本語", "モデルを選択", "Strataで使用"],
    ["简体中文", "选择模型", "在 Strata 中使用"],
  ]) {
    await language(page, label);
    await preferences(page, "engine");
    await page
      .getByRole("button", {
        name: label === "日本語" ? "ローカルモデル" : "本地模型",
        exact: true,
      })
      .click();
    await dialog.locator('.model-inventory[aria-busy="false"]').waitFor();
    await dialog
      .getByRole("button", {
        name: select + " · ready-fixture.gguf",
        exact: true,
      })
      .click();
    await dialog.getByRole("button", { name: use, exact: true }).waitFor();
    await page.keyboard.press("Escape");
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: real API model selection, missing-shard refusal, existing-config activation, supervised local preparation without model downloads, 3 languages, 4 responsive selection footers.",
  );
} finally {
  if (baseline) await put(baseline);
  await browser.close();
  try {
    const cfg = JSON.parse(
      await readFile(join(fixture, "strata-q2_0.json"), "utf8"),
    );
    const native = cfg.args[cfg.args.indexOf("--native") + 1];
    const target = await realpath(native);
    const workspace = dirname(dirname(native));
    if (
      target.startsWith(fixture + "/") &&
      /\/Strata-data\/local-models\/Q2_0-[0-9a-f]+$/.test(workspace)
    )
      await rm(workspace, { recursive: true, force: true });
  } catch {}
  await rm(fixture, { recursive: true, force: true });
}
