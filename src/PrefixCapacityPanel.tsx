import { useEffect, useRef, useState } from "react";
import { HardDrive, Info, RefreshCw, Trash2 } from "lucide-react";
import { request } from "./client";
import type { Language } from "./i18n";

type Capacity = {
  quotaBytes: number; targetBytes: number; usedBytes: number; cacheBytes: number;
  temporaryBytes: number; unknownBytes: number; entryCount: number;
  temporaryCount: number; unknownCount: number; canTrim: boolean;
};
type Report = { staging?: { bytes: number; directories: number; staleBytes: number; staleDirectories: number; otherBytes: number; otherEntries: number; cleanableBytes: number; cleanableDirectories: number }; stagingCleanup?: { removedBytes: number; removedFiles: number; removedDirectories: number; failedDirectories: number }; capacity: Capacity; diskFreeBytes: number | null; trim?: {
  removedBytes: number; removedEntries: number; targetMet: boolean; failedEntries: number;
} };
const text = {
  en: { cleanup: "Clean orphan staging", eligible: "Cleanable", cleaned: "Staging reclaimed", changed: "Some folders changed or could not be removed; refresh and retry.", staging: "Staging", stale: "Older than 24 h", stageCount: "Old / all folders", outside: "Other outside store", stagingHelp: "Logical file sizes outside the saved-store quota. Stale candidates require an old private stage directory and old recognized files; The cleanup button removes only stopped-engine candidates with a valid worker identity; recent, unknown or changed files stay protected. Refresh for a new snapshot. Unknown directories or links stop the scan.", title: "SSD capacity", refresh: "Refresh capacity", trim: "Trim cache", saved: "Saved", temporary: "Temporary", other: "Other", entries: "Entries", free: "Disk free", removed: "Reclaimed", partial: "Some files remain; target not reached.", unavailable: "Refresh to inspect the saved store.", help: "Snapshot of store files, including headers. While stopped, trim oldest saved states to 75% of quota and remove recognized temporary files older than 24 hours. Unknown files stay protected. Staging usage is shown separately below. Reads do not update saved-file age." },
  ja: { cleanup: "残存作業ファイルを整理", eligible: "整理可能", cleaned: "整理した作業領域", changed: "変更されたか削除できないフォルダーがあります。更新して再確認してください。", staging: "作業領域", stale: "24時間超の候補", stageCount: "候補 / 全フォルダー", outside: "保存領域外のその他", stagingHelp: "保存枠の外にあるファイルの論理サイズです。非公開の作業フォルダーと識別可能なファイルの両方が24時間以上経過した場合に候補とします。停止中の整理ボタンは、有効な識別情報のある候補だけを削除します。最近・不明・変更済みのファイルは保護します。更新時に再計測し、不明なフォルダーやリンクがあれば計測を中止します。", title: "SSD容量", refresh: "容量を更新", trim: "キャッシュを整理", saved: "保存済み", temporary: "一時ファイル", other: "その他", entries: "保存件数", free: "ディスク空き", removed: "整理した容量", partial: "残ったファイルがあり、目標容量に達していません。", unavailable: "更新して保存領域を確認できます。", help: "ヘッダーを含む保存領域の使用量です。停止中に古い保存から容量枠の75%まで整理し、24時間以上経過した識別可能な一時ファイルを削除します。不明なファイルは保護します。作業領域は下に別表示します。再利用しても保存日時は更新しません。" },
  zh: { cleanup: "清理残留暂存文件", eligible: "可清理", cleaned: "已回收暂存空间", changed: "部分目录已变更或无法删除，请刷新后重试。", staging: "暂存区域", stale: "超过24小时的候选", stageCount: "候选 / 全部目录", outside: "存储区外的其他文件", stagingHelp: "保存限额之外的文件逻辑大小。仅当私有暂存目录及其已识别文件均超过24小时，才列为候选；停止引擎后，清理按钮仅删除具有有效身份信息的候选。最近、未知或已更改的文件受到保护。刷新可重新测量，未知目录或链接会中止扫描。", title: "SSD容量", refresh: "刷新容量", trim: "整理缓存", saved: "已保存", temporary: "临时文件", other: "其他", entries: "保存条目", free: "磁盘可用", removed: "已回收", partial: "部分文件保留，尚未达到目标容量。", unavailable: "刷新以查看保存区域。", help: "包含文件头的存储用量快照。停止引擎后，按保存时间从旧到新整理至上限的75%，并删除超过24小时的可识别临时文件。未知文件受到保护。暂存用量在下方单独显示。读取不会更新保存时间。" },
};
const bytes = (n: number | null) => {
  if (n == null) return "—";
  const scale = n >= 1024 ** 4 ? 4 : n >= 1024 ** 3 ? 3 : n >= 1024 ** 2 ? 2 : n >= 1024 ? 1 : 0;
  return `${(n / 1024 ** scale).toLocaleString(undefined, { maximumFractionDigits: 1 })} ${["B", "KiB", "MiB", "GiB", "TiB"][scale]}`;
};
export function PrefixCapacityPanel({ language, locked, configurationKey, onSave }: {
  language: Language; locked: boolean; configurationKey: string; onSave: () => Promise<void>;
}) {
  const t = text[language];
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const pending = useRef<AbortController | null>(null);
  useEffect(() => {
    pending.current?.abort(); setReport(null); setError(""); setBusy(false);
    return () => pending.current?.abort();
  }, [configurationKey]);
  async function update(operation: "refresh" | "trim" | "cleanup") {
    pending.current?.abort(); const controller = new AbortController(); pending.current = controller;
    setBusy(true); setError("");
    try {
      if (!locked) await onSave();
      if (controller.signal.aborted) return;
      const reply = await request(operation === "refresh" ? "/studio/prefix-capacity" : "/studio/action", operation === "refresh" ? undefined : { action: operation === "trim" ? "trimPrefixCache" : "cleanupPrefixStaging" }, controller.signal);
      const value: Report = await reply.json();
      if (!controller.signal.aborted) setReport(value);
    } catch (e) { if (!controller.signal.aborted) { setReport(null); setError(String(e)); } }
    finally { if (!controller.signal.aborted) setBusy(false); }
  }
  const c = report?.capacity;
  return <section className="prefix-capacity" aria-label={t.title}>
    <div className="prefix-capacity-heading"><span><HardDrive size={14} /> {t.title}</span>
      <span title={t.help} aria-label={t.help} tabIndex={0}><Info size={14} /></span>
      <button type="button" className="icon-button" aria-label={t.refresh} title={t.refresh} disabled={busy} onClick={() => void update("refresh")}><RefreshCw size={14} /></button>
      <button type="button" className="icon-button" aria-label={t.trim} title={t.trim} disabled={locked || busy || !c?.canTrim} onClick={() => void update("trim")}><Trash2 size={14} /></button>
    </div>
    {c ? <><div className="prefix-capacity-total">{bytes(c.usedBytes)} / {bytes(c.quotaBytes)}</div>
      <progress aria-label={t.title} max={c.quotaBytes} value={Math.min(c.usedBytes, c.quotaBytes)} />
      <dl className="prefix-stats">
        {[[t.saved, bytes(c.cacheBytes)], [t.temporary, bytes(c.temporaryBytes)], [t.other, bytes(c.unknownBytes)], [t.entries, c.entryCount], [t.free, bytes(report.diskFreeBytes)]].map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
      </dl>
      {report.staging && <div className="prefix-staging">
        <div className="prefix-capacity-heading"><span>{t.staging}</span><span title={t.stagingHelp} aria-label={t.stagingHelp} tabIndex={0}><Info size={14} /></span><button type="button" className="icon-button" aria-label={t.cleanup} title={t.cleanup} disabled={locked || busy || !report.staging.cleanableDirectories} onClick={() => void update("cleanup")}><Trash2 size={14} /></button></div>
        <dl className="prefix-stats">
          {[[t.staging, bytes(report.staging.bytes)], [t.stale, bytes(report.staging.staleBytes)], [t.stageCount, `${report.staging.staleDirectories} / ${report.staging.directories}`], [t.outside, bytes(report.staging.otherBytes)], [t.eligible, bytes(report.staging.cleanableBytes)]].map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
        </dl>
      </div>}
      {report.stagingCleanup && <p role="status">{t.cleaned}: {bytes(report.stagingCleanup.removedBytes)}{report.stagingCleanup.failedDirectories > 0 ? ` · ${t.changed}` : ""}</p>}
      {report.trim && <p role="status">{t.removed}: {bytes(report.trim.removedBytes)}{!report.trim.targetMet || report.trim.failedEntries > 0 ? ` · ${t.partial}` : ""}</p>}
    </> : <p className="knowledge-hint">{t.unavailable}</p>}
    {error && <p role="alert" className="knowledge-error">{error}</p>}
  </section>;
}
