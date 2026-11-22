import { preferences, closePreferences, language } from "./ui-helpers.mjs";
import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { tmpdir } from "node:os";
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
});
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});
const folder = await mkdtemp(join(tmpdir(), "soliton-prompt-folder-"));
try {
  await page.goto(process.env.SOLITON_TEST_URL || "http://127.0.0.1:5180");
  await language(page, "English");
  await preferences(page, "engine");
  await page
    .getByRole("button", { name: "Start", exact: true })
    .waitFor({ state: "visible" });
  await closePreferences(page);
  await page.getByRole("button", { name: "New conversation" }).click();
  await page.waitForFunction(
    () =>
      !document.querySelector('button[aria-label="New conversation"]').disabled,
  );
  await page.screenshot({ path: "/tmp/soliton-desktop.png" });
  assert.equal(await page.title(), "Soliton");
  await preferences(page, "engine");
  await page.getByText("Advanced settings", { exact: true }).click();
  await page
    .getByRole("textbox", { name: "Python path", exact: true })
    .fill(process.env.STUDIO_TEST_PYTHON || "python3");
  await page.getByRole("checkbox", { name: "Test engine" }).check();
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await page.waitForFunction(
    () => document.querySelector(".value-row .status")?.textContent === "Ready",
    { timeout: 30000 },
  );
  await closePreferences(page);
  await page
    .locator('.composer input[type="file"]:not([webkitdirectory])')
    .setInputFiles([
      {
        name: "keep.md",
        mimeType: "text/markdown",
        buffer: Buffer.from("Handoff code 7429"),
      },
      {
        name: "remove.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("This must stay local"),
      },
    ]);
  await page.getByRole("button", { name: "Remove file · remove.txt" }).click();
  await writeFile(join(folder, "chosen.txt"), "Folder reference 8316");
  await writeFile(join(folder, "excluded.txt"), "Never include this text");
  await page.locator(".composer input[webkitdirectory]").setInputFiles(folder);
  const listing = page.getByRole("region", { name: "Choose folder" });
  const chosen = listing.getByRole("checkbox", { name: /chosen\.txt$/ });
  assert.equal(await chosen.isChecked(), false);
  await chosen.check();
  await listing.getByRole("button", { name: "Attach selected (1)" }).click();
  await listing.waitFor({ state: "hidden" });
  const prompt = page.getByRole("textbox", { name: "Type a message…" });
  await prompt.fill("Hello Strata");
  const budgetButton = page.getByRole("button", { name: "Check token budget" });
  assert.equal(await budgetButton.count(), 0);
  // Simulate an exact-count-capable engine for the UI portion of the smoke.
  await page.route("**/studio/state", async (route) => {
    const response = await route.fetch();
    const state = await response.json();
    state.backend = "Inkling Metal (experimental)";
    await route.fulfill({ response, json: state });
  });
  let previewPayload;
  await page.route("**/studio/chat/budget", async (route) => {
    previewPayload = route.request().postDataJSON();
    await route.fulfill({
      json: {
        inputTokens: 4000,
        outputTokens: 128,
        context: 4096,
        fits: false,
      },
    });
  });
  await budgetButton.click();
  await page.getByText("Over limit", { exact: false }).waitFor();
  assert.match(previewPayload.content, /Handoff code 7429/);
  assert.match(previewPayload.content, /Folder reference 8316/);
  assert.doesNotMatch(previewPayload.content, /Never include this text/);
  await prompt.fill("Changed draft");
  await page
    .getByText("Over limit", { exact: false })
    .waitFor({ state: "hidden" });
  await page.unroute("**/studio/state");
  await page.unroute("**/studio/chat/budget");
  await prompt.fill("Hello Strata");
  await prompt.press("Enter");
  assert.equal(await prompt.inputValue(), "Hello Strata\n");
  await prompt.press("Shift+Enter");
  await page.waitForFunction(
    () =>
      document.querySelector(".message.assistant p")?.textContent.length > 5,
  );
  await page
    .getByRole("button", { name: "New conversation" })
    .waitFor({ state: "visible" });
  await page.waitForFunction(
    () =>
      !document.querySelector('button[aria-label="New conversation"]').disabled,
  );
  const conn = await page.evaluate(async () =>
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
  const shared = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/studio/state",
    {
      headers,
    },
  ).then((r) => r.json());
  assert.equal(shared.document.messages.length, 2);
  assert.equal(
    shared.document.messages[0].content,
    `Hello Strata\n\n--- "keep.md" ---\nHandoff code 7429\n\n--- "${basename(folder)}/chosen.txt" ---\nFolder reference 8316`,
  );
  assert.equal(
    shared.document.messages[1].content,
    "Hello from the mock engine.",
  );
  assert.equal(shared.document.generating, false);

  const models = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/v1/models",
    {
      headers,
    },
  ).then((r) => r.json());
  assert(models.data.length > 0);
  const anthropic = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") + "/v1/messages",
    {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: models.data[0].id,
        max_tokens: 32,
        messages: [{ role: "user", content: "Hello" }],
      }),
    },
  );
  assert.equal(anthropic.status, 200);
  assert((await anthropic.json()).content.length > 0);
  const streamed = await fetch(
    (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
      "/v1/chat/completions",
    {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: models.data[0].id,
        max_tokens: 32,
        stream: true,
        messages: [{ role: "user", content: "Hello" }],
      }),
    },
  );
  assert.equal(streamed.status, 200);
  assert((await streamed.text()).includes("[DONE]"));
  assert.equal(
    (
      await fetch(
        (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
          "/studio/state",
      )
    ).status,
    401,
  );
  assert.equal(
    (
      await fetch(
        (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
          "/v1/models",
      )
    ).status,
    401,
  );
  assert.equal(
    (
      await fetch(
        (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
          "/studio/session",
        {
          method: "POST",
          headers: { Origin: "https://example.com" },
        },
      )
    ).status,
    403,
  );
  await page.screenshot({ path: "/tmp/soliton-chat.png" });
  await preferences(page, "engine");
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.waitForFunction(
    () =>
      document.querySelector(".value-row .status")?.textContent === "Offline",
  );
  assert.equal(
    (
      await fetch(
        (process.env.SOLITON_TEST_API || "http://127.0.0.1:8080") +
          "/v1/models",
        { headers },
      )
    ).status,
    503,
  );
  await closePreferences(page);
  await page.getByRole("button", { name: "New conversation" }).click();
  await language(page, "简体中文");
  await closePreferences(page);
  await page.getByText("从这里开始推理。", { exact: true }).waitFor();
  await language(page, "日本語");
  await closePreferences(page);
  await page.getByText("推論を、ここから。", { exact: true }).waitFor();
  for (const width of [320, 375, 414, 768, 1440, 1536]) {
    await page.setViewportSize({ width, height: width === 1536 ? 1024 : 960 });
    assert(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      "Overflow at " + width,
    );
    const boxes = await page
      .locator("input,textarea,button")
      .evaluateAll((es) =>
        es
          .filter((e) => e.getBoundingClientRect().width > 0)
          .map((e) => {
            const b = e.getBoundingClientRect();
            return { left: b.left, right: b.right };
          }),
      );
    assert(
      boxes.every((b) => b.left >= 0 && b.right <= width),
      "Control clipping at " + width,
    );
    if (width === 1536)
      await page.screenshot({ path: "/tmp/soliton-native-size.png" });
    if (width === 375)
      await page.screenshot({
        path: "/tmp/soliton-mobile.png",
        fullPage: true,
      });
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: UI lifecycle, streamed chat, OpenAI models/SSE, Anthropic, auth, offline 503, three languages, six viewport widths, no runtime errors.",
  );
} finally {
  await browser.close();
  await rm(folder, { recursive: true, force: true });
}
