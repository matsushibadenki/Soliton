export async function preferences(page, tab) {
  if (!(await page.locator(".preferences-dialog[open]").count())) {
    await page
      .getByRole("menubar")
      .getByRole("menuitem", { name: "Soliton", exact: true })
      .click();
    await page
      .getByRole("menu")
      .getByRole("menuitem", { name: /Preferences|環境設定|偏好设置/ })
      .click();
  }
  if (tab) await page.locator("#preference-" + tab).click();
}
export async function closePreferences(page) {
  await page
    .locator(".preferences-dialog")
    .getByRole("button", { name: /^(Close|閉じる|关闭)$/ })
    .click();
  await page.locator(".preferences-dialog").waitFor({ state: "hidden" });
}
export async function language(page, label) {
  await preferences(page, "general");
  await page.getByRole("button", { name: label, exact: true }).click();
}
