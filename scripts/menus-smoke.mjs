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
  const statusTab = page.getByRole("tab", { name: "Status", exact: true });
  assert.equal(await statusTab.getAttribute("aria-selected"), "true");
  assert(await page.locator(".preferences-dialog .memory-usage").isVisible());
  assert(await page.locator(".preferences-dialog .memory-history").isVisible());
  assert.equal(await page.getByRole("combobox", { name: "Inference engine", exact: true }).count(), 0);
  await statusTab.press("ArrowRight");
  assert.equal(await page.locator(".preferences-dialog .memory-usage").count(), 0);
  await page.locator(".preferences-dialog .engine-details > summary").click();
  assert(
    await page
      .getByRole("textbox", { name: "Python path", exact: true })
      .isVisible(),
  );
  await page
    .getByRole("tab", { name: "Engine settings", exact: true })
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
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 960 });
    const bounds = await page.locator(".workspace .panel.chat").boundingBox();
    assert(bounds && Math.abs(bounds.x) <= 1 && Math.abs(bounds.x + bounds.width - width) <= 1);
  }
  await page.setViewportSize({ width: 1440, height: 960 });
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
    const content = page.locator(".conversation");
    await content.getByText("Export fixture", { exact: true }).waitFor();
    const sidebar = page.getByRole("complementary", { name: "Conversations" });
    const toggle = page.locator("button[aria-controls=conversation-sidebar]");
    assert.equal(await toggle.count(), 1);
    assert.equal(await toggle.evaluate((element) => element.nextElementSibling?.getAttribute("aria-label")), "Preferences…");
    assert.equal(await sidebar.getByRole("button", { name: "Hide conversation history" }).count(), 0);
    const sidebarBounds = await sidebar.boundingBox();
    const contentBounds = await content.boundingBox();
    assert(sidebarBounds.x + sidebarBounds.width <= contentBounds.x + 1);
    await toggle.click();
    await sidebar.waitFor({ state: "hidden" });
    assert.equal(await toggle.getAttribute("aria-expanded"), "false");
    await toggle.click();
    await sidebar.waitFor({ state: "visible" });
    await sidebar.getByRole("button", { name: "New conversation" }).click();
    await content.getByText("Export fixture", { exact: true }).waitFor({ state: "hidden" });
    const search = sidebar.getByRole("searchbox");
    await search.fill("Export fixture");
    await sidebar.getByRole("button", { name: /^Export fixture/ }).click();
    await content.getByText("Export fixture", { exact: true }).waitFor();
    await search.fill("");
    await page.setViewportSize({ width: 375, height: 720 });
    assert(await sidebar.isVisible() && await composer.isVisible());
    await search.press("Escape");
    await sidebar.waitFor({ state: "hidden" });
    assert(await toggle.evaluate((element) => element === document.activeElement));
    await page.setViewportSize({ width: 1440, height: 960 });
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
    const longPrompt = "Wide user message ".repeat(90).trim();
    assert.equal((await put([{ role: "user", content: longPrompt }])).status, 200);
    const bubble = content.locator(".message.user");
    await bubble.getByText(longPrompt, { exact: true }).waitFor();
    for (const width of [1440, 375]) {
      await page.setViewportSize({ width, height: 720 });
      const geometry = await bubble.evaluate((element) => {
        const parent = element.parentElement;
        const layout = getComputedStyle(parent);
        const left = Number.parseFloat(layout.paddingLeft);
        const right = Number.parseFloat(layout.paddingRight);
        return {
          width: parent.clientWidth - left - right,
          edge: parent.getBoundingClientRect().left + parent.clientLeft + parent.clientWidth - right,
          background: getComputedStyle(element).backgroundColor,
        };
      });
      const bounds = await bubble.boundingBox();
      assert(Math.abs(bounds.width - geometry.width * 2 / 3) <= 1);
      assert(Math.abs(bounds.x + bounds.width - geometry.edge) <= 1);
      assert.notEqual(geometry.background, "rgba(0, 0, 0, 0)");
    }
    await page.setViewportSize({ width: 1440, height: 960 });
    const markdown = [
      "## Markdown fixture",
      "",
      "**Bold** and ~~old~~, with $x^2$ and \\(\\frac{1}{2}\\).",
      "",
      "| Item | Value |",
      "| --- | ---: |",
      "| Result | 42 |",
      "",
      "- [x] Verified",
      "- [ ] Follow-up",
      "",
      "> A quotation",
      "",
      "\\[\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\\]",
      "",
      "$$\\sum_{i=1}^n i$$",
      "",
      "Prices: $5 and $7. A note.[^note]",
      "",
      "```bash",
      "echo '\\(literal\\)'",
      "```",
      "",
      "[unsafe](javascript:alert%281%29) <img src=x onerror=alert(1)>",
      "",
      "[^note]: Footnote content",
    ].join("\n");
    assert.equal((await put([{ role: "assistant", content: markdown }])).status, 200);
    const rendered = content.locator(".chat-message-content");
    await rendered.getByRole("heading", { name: "Markdown fixture" }).waitFor();
    assert.equal(await rendered.locator("table tr").count(), 2);
    assert.equal(await rendered.locator("strong").innerText(), "Bold");
    assert.equal(await rendered.locator("del").innerText(), "old");
    assert.equal(await rendered.locator("blockquote").innerText(), "A quotation");
    assert.equal(await rendered.locator("input[type=checkbox]:checked").count(), 1);
    assert.equal(await rendered.locator(".chat-math-inline math").count(), 2);
    assert.equal(await rendered.locator(".chat-math-display math").count(), 2);
    assert.match(await rendered.locator("pre code").innerText(), /literal/);
    assert.match(await rendered.innerText(), /Prices: \$5 and \$7/);
    assert.equal(await rendered.locator("a").filter({ hasText: "unsafe" }).getAttribute("href"), null);
    assert.equal(await rendered.locator("img").count(), 0);
    assert(await rendered.getByText("Footnote content", { exact: false }).isVisible());
    await page.setViewportSize({ width: 375, height: 720 });
    const overflow = await rendered.evaluate((element) => {
      if (element.scrollWidth <= element.clientWidth + 1) return [];
      const edge = element.getBoundingClientRect().right;
      return Array.from(element.querySelectorAll("*"))
        .filter((child) => child.getBoundingClientRect().right > edge + 1)
        .slice(0, 12)
        .map((child) => [child.tagName, child.getAttribute("class")]);
    });
    assert.deepEqual(overflow, []);
    await page.setViewportSize({ width: 1440, height: 960 });
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
