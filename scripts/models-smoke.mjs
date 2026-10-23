// Browser plugin not available: verify the local UI with Playwright.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { preferences, closePreferences, language } from "./ui-helpers.mjs";
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
});
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
});
const fixture = await mkdtemp(join(tmpdir(), "strata-model-ui-"));
let baseline, headers;
const state = () =>
  fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/studio/state",
    { headers },
  ).then((r) => r.json());
async function ready() {
  await page
    .locator('.model-inventory[aria-busy="false"]')
    .waitFor({ timeout: 60000 });
}
try {
  const buffer = Buffer.alloc(24);
  buffer.write("GGUF");
  buffer.writeUInt32LE(3, 4);
  await writeFile(join(fixture, "Strata-inventory-fixture.gguf"), buffer);
  await writeFile(join(fixture, "Strata-invalid-fixture.gguf"), "incomplete");
  await writeFile(
    join(fixture, "Strata-tensor-fixture.safetensors"),
    "fixture",
  );
  await writeFile(join(fixture, "ignored.gguf.part"), "partial");
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
  baseline = (await state()).settings;
  assert.equal(
    (
      await fetch(
        (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
          "/studio/models",
      )
    ).status,
    401,
  );
  await language(page, "English");
  await preferences(page, "engine");
  await page.getByRole("button", { name: "Local models", exact: true }).click();
  await ready();
  const dialog = page.locator(".model-library-dialog");
  await dialog
    .getByRole("textbox", { name: "Custom folders", exact: true })
    .fill(fixture);
  await dialog.getByRole("button", { name: "Add folder", exact: true }).click();
  await ready();
  await dialog
    .locator(".model-source-filters")
    .getByRole("button", { name: /^Custom/ })
    .click();
  await dialog
    .getByText("Strata-inventory-fixture.gguf", { exact: true })
    .waitFor();
  assert.equal(await dialog.locator("article.model-file").count(), 3);
  assert(
    await dialog.getByText("Invalid model header", { exact: true }).isVisible(),
  );
  assert(
    await dialog
      .getByText("Not supported directly by Strata", { exact: true })
      .isVisible(),
  );
  assert((await state()).settings.modelDirectories.includes(fixture));
  await dialog
    .getByRole("textbox", { name: "Filter models…", exact: true })
    .fill("Strata-inventory");
  assert.equal(await dialog.locator("article.model-file").count(), 1);
  await dialog
    .getByRole("textbox", { name: "Filter models…", exact: true })
    .fill("");
  await dialog.getByText("Search locations", { exact: false }).first().click();
  const scan = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
      "/studio/models",
    {
      headers,
    },
  ).then((r) => r.json());
  for (const source of ["strata", "lmstudio", "unsloth", "ollama", "custom"])
    assert(scan.roots.some((r) => r.source === source));
  assert(
    scan.files.some(
      (m) =>
        m.name === "Strata-inventory-fixture.gguf" &&
        m.sources.includes("custom"),
    ),
  );
  await page.screenshot({ path: "/tmp/strata-model-library.png" });
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
        .locator(".model-library-body")
        .evaluate((e) => e.scrollWidth <= e.clientWidth),
      "Horizontal content overflow " + width,
    );
    await dialog.screenshot({ path: `/tmp/strata-model-library-${width}.png` });
  }
  await page.reload();
  await preferences(page, "engine");
  await page.getByRole("button", { name: "Local models", exact: true }).click();
  await ready();
  assert(
    await page
      .locator(".custom-folder-list")
      .getByText(fixture, { exact: true })
      .isVisible(),
  );
  await dialog
    .getByRole("button", { name: "Remove folder · " + fixture, exact: true })
    .click();
  await ready();
  assert(!(await state()).settings.modelDirectories.includes(fixture));
  for (const [label, title] of [
    ["日本語", "ローカルモデル"],
    ["简体中文", "本地模型"],
  ]) {
    await page.keyboard.press("Escape");
    await closePreferences(page);
    await language(page, label);
    await preferences(page, "engine");
    await page.getByRole("button", { name: title, exact: true }).click();
    await ready();
    assert(
      await dialog
        .getByRole("heading", { name: title, exact: true })
        .isVisible(),
    );
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: authenticated five-source scan, custom folder add/remove and persistence, GGUF/unsupported/invalid classification, search, 3 languages, 4 widths, no runtime errors.",
  );
} finally {
  if (baseline && headers)
    await fetch(
      (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
        "/studio/action",
      {
        method: "POST",
        headers,
        body: JSON.stringify({ action: "settings", settings: baseline }),
      },
    );
  await browser.close();
  await rm(fixture, { recursive: true, force: true });
}
