// Browser plugin not available: exercise our local application with Playwright.
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
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
async function menu(name) {
  await page
    .getByRole("menubar")
    .getByRole("menuitem", { name, exact: true })
    .click();
}
try {
  await page.goto(process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180");
  await language(page, "English");
  await closePreferences(page);
  assert.equal(await page.title(), "Soliton");
  assert.equal(
    await page.getByRole("button", { name: "English", exact: true }).count(),
    0,
  );
  assert.equal(await page.locator(".engine").count(), 0);
  await menu("Soliton");
  await page
    .getByRole("menuitem", { name: "About Soliton", exact: true })
    .click();
  assert(
    await page
      .getByRole("dialog")
      .getByText("0.1.0", { exact: true })
      .isVisible(),
  );
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+,");
  assert(await page.locator(".preferences-dialog[open]").isVisible());
  await page
    .getByRole("tab", { name: "General", exact: true })
    .press("ArrowRight");
  assert(
    await page
      .getByRole("textbox", { name: "Python path", exact: true })
      .isVisible(),
  );
  await page
    .getByRole("tab", { name: "Engine", exact: true })
    .press("ArrowRight");
  assert(await page.locator(".preferences-dialog .api").isVisible());
  await closePreferences(page);
  const composer = page.locator(".composer textarea");
  await composer.fill("Menu selection check");
  await menu("Edit");
  await page.getByRole("menuitem", { name: "Select All", exact: true }).click();
  assert.deepEqual(
    await composer.evaluate((e) => [e.selectionStart, e.selectionEnd]),
    [0, 20],
  );
  await composer.press("Backspace");
  assert.equal(await composer.inputValue(), "");
  await menu("Edit");
  await page.getByRole("menuitem", { name: "Undo", exact: true }).click();
  assert.equal(await composer.inputValue(), "Menu selection check");
  await menu("File");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Escape");
  assert.equal(await page.getByRole("menu").count(), 0);
  await menu("View");
  await page.getByRole("menuitemradio", { name: "Logs", exact: true }).click();
  await page.locator(".log-lines").waitFor({ state: "visible" });
  await page.getByRole("tab", { name: "Logs", exact: true }).press("ArrowLeft");
  await composer.waitFor({ state: "visible" });
  await page.screenshot({ path: "/tmp/strata-menu-main.png" });
  // Export uses an isolated fixture conversation and restores the Rust document.
  const conn = await page.evaluate(() =>
    fetch("/studio/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    }).then((r) => r.json()),
  );
  const headers = {
    "Content-Type": "application/json",
    "x-studio-token": conn.token,
  };
  const initial = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/studio/state",
    {
      headers,
    },
  ).then((r) => r.json());
  const fixture = [{ role: "user", content: "Export fixture" }];
  const put = (messages) =>
    fetch(
      (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
        "/studio/action",
      {
        method: "POST",
        headers,
        body: JSON.stringify({ action: "messages", messages }),
      },
    );
  try {
    assert.equal((await put(fixture)).status, 200);
    await page.getByText("Export fixture", { exact: true }).waitFor();
    await menu("File");
    const downloaded = page.waitForEvent("download");
    await page
      .getByRole("menuitem", { name: "Export conversation…", exact: true })
      .click();
    const download = await downloaded;
    assert.equal(download.suggestedFilename(), "Soliton-conversation.json");
    assert.deepEqual(
      JSON.parse(await readFile(await download.path(), "utf8")).messages,
      fixture,
    );
  } finally {
    await put(initial.document.messages);
  }
  for (const [label, names] of [
    ["日本語", ["ファイル", "編集", "表示", "ヘルプ"]],
    ["简体中文", ["文件", "编辑", "视图", "帮助"]],
    ["English", ["File", "Edit", "View", "Help"]],
  ]) {
    await language(page, label);
    await closePreferences(page);
    for (const name of names)
      assert(
        await page
          .getByRole("menubar")
          .getByRole("menuitem", { name, exact: true })
          .isVisible(),
      );
    for (const width of [320, 375, 768, 1440]) {
      await page.setViewportSize({ width, height: 720 });
      assert(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      );
      await menu(names[3]);
      const menuBox = await page.getByRole("menu").boundingBox();
      assert(
        menuBox.x >= 0 && menuBox.x + menuBox.width <= width,
        JSON.stringify({ label, width, menuBox }),
      );
      await page.keyboard.press("Escape");
      await preferences(page, "engine");
      assert(
        await page.locator(".preferences-dialog").evaluate((e) => {
          const b = e.getBoundingClientRect();
          return (
            b.left >= 0 && b.right <= innerWidth && b.bottom <= innerHeight
          );
        }),
      );
      assert(
        await page
          .getByRole("button", { name: /^(Start|起動|启动)$/ })
          .isVisible(),
      );
      await page
        .locator(".preferences-dialog")
        .screenshot({ path: `/tmp/strata-preferences-${width}.png` });
      await closePreferences(page);
    }
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: logo/about, preferences tabs/shortcuts, edit selection/undo, menu keyboard dismissal, workspace tabs, conversation export, 3 locales and 4 widths.",
  );
} finally {
  await browser.close();
}
