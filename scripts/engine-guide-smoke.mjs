import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "@playwright/test";
import { preferences } from "./ui-helpers.mjs";

const dir = await mkdtemp(join(tmpdir(), "soliton-guide-"));
const port = process.env.STUDIO_TEST_PORT || "19358";
const origin = "http://127.0.0.1:" + port;
const server = spawn(resolve("target/debug/studio-server"), {
  env: {
    ...process.env,
    STUDIO_PORT: port,
    STUDIO_STATE_PATH: join(dir, "studio.json"),
  },
});
let browser;
try {
  const fixture = "LFM3-Omni-2B-Q4_K_M.gguf";
  const header = Buffer.alloc(24);
  header.write("GGUF");
  header.writeUInt32LE(3, 4);
  await writeFile(join(dir, fixture), header);
  // Small catalog fixtures exercise model identification without loading weights.
  for (const [folder, model_type, hidden_size] of [
    ["gemma-two", "gemma2", 2304],
    ["gemma-three", "gemma3_text", 1152],
  ]) {
    await mkdir(join(dir, folder));
    await writeFile(
      join(dir, folder, "config.json"),
      JSON.stringify({ model_type, hidden_size, num_hidden_layers: 26 }),
    );
    await writeFile(join(dir, folder, "model.safetensors"), Buffer.alloc(16));
  }
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) throw Error("Isolated server exited");
    if (
      await fetch(origin + "/health")
        .then((r) => r.ok)
        .catch(() => false)
    )
      break;
    await new Promise((r) => setTimeout(r, 50));
  }
  browser = await chromium.launch({
    executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
  });
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(origin);
  await preferences(page, "engine");
  const pref = page.locator(".preferences-dialog .engine-guide");
  for (const [model, task, expected] of [
    ["Gemma 3 1B IT", "text", "Strata"],
    ["Gemma 2 2B", "text", "MLX SSD"],
    ["LFM3-Omni-2B", "voice", "LFM3-Omni"],
  ]) {
    await pref.locator("select").first().selectOption(model);
    await pref.locator("select").last().selectOption(task);
    assert((await pref.innerText()).includes(expected));
  }
  const state = () =>
    page.evaluate(async () => {
      const { token } = await (
        await fetch("/studio/session", { method: "POST" })
      ).json();
      const response = await fetch("/studio/state", {
        headers: { "x-studio-token": token },
      });
      if (!response.ok) throw Error("Could not read server state");
      const { settings, selectedEngine, document } = await response.json();
      return {
        settings,
        selectedEngine,
        conversations: document.conversations,
      };
    });
  const before = await state();
  await pref.locator("select").first().selectOption("Gemma 2 2B");
  assert.equal(await pref.locator(".engine-guide-row").count(), 0);
  await pref.locator("select").first().selectOption("LFM3-Omni-2B");
  const expected = await pref.locator(".engine-guide-row").innerText();
  const gemma = [];
  for (const [folder, model] of [
    ["gemma-two", "Gemma 2 2B"],
    ["gemma-three", "Gemma 3 1B IT"],
  ]) {
    await pref.locator("select").first().selectOption(model);
    await pref.locator("select").last().selectOption("text");
    gemma.push([
      folder,
      await pref.locator(".engine-guide-row").allInnerTexts(),
    ]);
  }
  assert.deepEqual(await state(), before);
  const manual = page.getByRole("combobox", {
    name: /^(Inference engine|推論エンジン|推理引擎)$/,
  });
  for (const engine of ["vllmApple", before.selectedEngine]) {
    const switched = page.waitForResponse(
      (r) =>
        r.url().endsWith("/studio/action") &&
        r.request().postDataJSON()?.action === "switchEngine",
    );
    await manual.selectOption(engine);
    assert((await switched).ok());
    const snapshot = await state();
    assert.equal(snapshot.selectedEngine, engine);
    assert.deepEqual(snapshot.conversations, before.conversations);
  }

  await page
    .getByRole("button", { name: /^(Local models|ローカルモデル|本地模型)$/ })
    .click();
  const library = page.locator(".model-library-dialog");
  await library
    .getByRole("textbox", {
      name: /Custom folders|ユーザー指定フォルダー|自定义文件夹/,
    })
    .fill(dir);
  await library
    .getByRole("button", { name: /Add folder|フォルダーを追加|添加文件夹/ })
    .click();
  await library
    .locator(".model-file")
    .filter({ hasText: fixture })
    .first()
    .getByRole("button")
    .click();
  const guide = library.locator(".engine-guide");
  await guide.locator("select").selectOption("voice");
  assert.equal(await guide.locator(".engine-guide-row").innerText(), expected);
  for (const [folder, rows] of gemma) {
    await library
      .locator(".model-file")
      .filter({ hasText: join(dir, folder) })
      .first()
      .getByRole("button")
      .click();
    await guide.locator("select").selectOption("text");
    await guide.locator(".engine-guide-row").first().waitFor();
    assert.deepEqual(
      await guide.locator(".engine-guide-row").allInnerTexts(),
      rows,
    );
  }
  const after = await state();
  assert.equal(after.selectedEngine, before.selectedEngine);
  assert.deepEqual(after.conversations, before.conversations);
  assert.equal(after.settings.engine, before.settings.engine);
  assert.deepEqual(errors, []);
  console.log(
    "Strata, MLX and Omni profiles match in Preferences and Local Models",
  );
} finally {
  await browser?.close();
  if (server.exitCode === null) {
    const exit = once(server, "exit");
    server.kill("SIGINT");
    await exit;
  }
  await rm(dir, { recursive: true, force: true });
}
