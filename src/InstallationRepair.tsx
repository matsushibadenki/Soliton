import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Modal } from "./PreferencesDialog";
import { desktop } from "./client";
import type { Language } from "./i18n";
export type InstallFailure = { code: string; path: string; detail: string; requiredBytes: number; availableBytes?: number };
export const repairWords = {
  ja: { title: "実行環境の設置", space: "設置用の空き容量が不足しています", permission: "保存先に書き込めません", installation: "実行環境を設置できませんでした", spaceHelp: "保存先のディスクに2 GiB以上の空きを確保してから再試行してください。モデルには別途容量が必要です。", permissionHelp: "Finderの「情報を見る」で保存先のアクセス権とディスクの読み取り専用状態を確認してください。", help: "空き容量とアクセス権を確認してください。改善しない場合はアプリを再取得してください。", retry: "再試行", busy: "設置中…", path: "保存先", details: "詳細", helpTitle: "対処方法" },
  en: { title: "Runtime installation", space: "Not enough installation space", permission: "Cannot write to the installation folder", installation: "Runtime installation failed", spaceHelp: "Free at least 2 GiB on the destination disk, then retry. Models require additional space.", permissionHelp: "Check the folder permissions in Finder’s Get Info and whether the disk is read-only.", help: "Check free space and folder permissions. Download the app again if the problem persists.", retry: "Retry", busy: "Installing…", path: "Destination", details: "Details", helpTitle: "How to resolve" },
  zh: { title: "运行环境安装", space: "安装空间不足", permission: "无法写入安装文件夹", installation: "无法安装运行环境", spaceHelp: "请在目标磁盘上释放至少2 GiB空间，然后重试。模型需要额外空间。", permissionHelp: "请在Finder的“显示简介”中检查文件夹权限，并确认磁盘不是只读状态。", help: "请检查可用空间和文件夹权限。如果问题仍然存在，请重新下载应用。", retry: "重试", busy: "正在安装…", path: "安装位置", details: "详细信息", helpTitle: "解决方法" },
};
export function InstallationRepair({ language }: { language: Language }) {
  const [failure, setFailure] = useState<InstallFailure | null>(null);
  const [busy, setBusy] = useState(false);
  const [retryError, setRetryError] = useState("");
  useEffect(() => { if (desktop) void invoke<InstallFailure | null>("installation_status").then(setFailure).catch((e) => setRetryError(String(e))); }, []);
  if (!failure) return null;
  const t = repairWords[language];
  const code = failure.code === "space" || failure.code === "permission" ? failure.code : "installation";
  return <Modal title={t.title} language={language} onClose={() => {}} className="installation-repair-dialog" dismissible={false}>
    <section style={{ padding: 16, maxWidth: 560, overflowWrap: "anywhere" }}>
      <p role="alert">{t[code]}</p>
      <p>{code === "space" ? t.spaceHelp : code === "permission" ? t.permissionHelp : t.help}</p>
      <p>{t.path}: <code>{failure.path}</code></p>
      <details><summary>{t.details}</summary><pre style={{ whiteSpace: "pre-wrap" }}>{failure.detail}</pre></details>
      {retryError && <p role="alert">{retryError}</p>}
      <button disabled={busy} onClick={async () => {
        setBusy(true); setRetryError("");
        try { await invoke("retry_installation"); location.reload(); }
        catch (e) {
          try { setFailure(await invoke<InstallFailure | null>("installation_status")); }
          catch { setRetryError(String(e)); }
        } finally { setBusy(false); }
      }}>{busy ? t.busy : t.retry}</button>
    </section>
  </Modal>;
}
