import { useEffect, useRef, useState } from "react";
import { FolderOpen, Info, RefreshCw } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { desktop, request } from "./client";
import type { Language } from "./i18n";
import { PrefixCapacityPanel } from "./PrefixCapacityPanel";
export type PrefixSettings = {
  python: string;
  model: string;
  runtimeDir: string;
  storeExecutable: string;
  cacheRoot: string;
  quotaMiB: number;
  evidence: string;
};
export const defaultPrefixSettings: PrefixSettings = {
  python: "",
  model: "",
  runtimeDir: "",
  storeExecutable: "",
  cacheRoot: "",
  quotaMiB: 256,
  evidence: "",
};
const labels = {
  en: {
    model: "MLX model folder",
    python: "MLX Python",
    runtimeDir: "SSD runtime scripts",
    storeExecutable: "Rust prefix-store executable",
    cacheRoot: "Private SSD cache folder",
    evidence: "SSD qualification evidence",
    quotaMiB: "SSD quota (MiB)",
    help: "Qualified Gemma 2 2B text profile: 512 context tokens including output, greedy seed 0, maximum 128 output tokens, one request at a time. Chat uses these fixed limits. A fresh 30-minute identity-bound SSD qualification is required; vLLM-Apple evidence cannot be reused. Stop before changing paths.",
    refresh: "Refresh SSD metrics",
    hits: "SSD hits",
    misses: "SSD misses",
    writes: "SSD writes",
    skippedWrites: "Repeated writes skipped",
    errors: "SSD storage errors",
    unavailable: "Start a qualified MLX SSD engine to read metrics.",
  },
  ja: {
    model: "MLXモデルフォルダー",
    python: "MLX用Python",
    runtimeDir: "SSD実行スクリプト",
    storeExecutable: "Rust prefix-store実行ファイル",
    cacheRoot: "非公開のSSDキャッシュフォルダー",
    evidence: "SSD構成の資格証跡",
    quotaMiB: "SSD容量枠（MiB）",
    help: "対象はGemma 2 2Bのテキスト構成です。入力と出力の合計512トークン、temperature 0・seed 0、出力最大128トークン、同時実行1件で動作します。チャットもこの固定条件を使います。構成と一致する新しい30分資格証跡が必要で、vLLM-Appleの証跡は流用できません。パスの変更前に停止してください。",
    refresh: "SSD計測を更新",
    hits: "SSD再利用",
    misses: "SSD未検出",
    writes: "SSD保存",
    skippedWrites: "再保存を省略",
    errors: "SSD保存エラー",
    unavailable: "資格確認済みのMLX SSDを起動すると計測できます。",
  },
  zh: {
    model: "MLX模型文件夹",
    python: "MLX Python",
    runtimeDir: "SSD运行脚本",
    storeExecutable: "Rust prefix-store可执行文件",
    cacheRoot: "私有SSD缓存文件夹",
    evidence: "SSD配置验证证据",
    quotaMiB: "SSD容量上限（MiB）",
    help: "仅支持Gemma 2 2B文本：输入加输出共512个令牌、temperature 0和seed 0、最多128个输出令牌、单并发。聊天也使用这些固定条件。需要匹配配置的新30分钟验证证据，不能复用vLLM-Apple证据。修改路径前请停止引擎。",
    refresh: "刷新SSD指标",
    hits: "SSD命中",
    misses: "SSD未命中",
    writes: "SSD写入",
    skippedWrites: "已跳过重复写入",
    errors: "SSD存储错误",
    unavailable: "启动已验证的MLX SSD引擎以查看指标。",
  },
};
export function PrefixEngineSettings({
  value,
  language,
  locked,
  running,
  onChange,
  onSave,
}: {
  value: PrefixSettings;
  language: Language;
  locked: boolean;
  running: boolean;
  onChange: (v: PrefixSettings) => void;
  onSave: () => Promise<void>;
}) {
  const t = labels[language];
  const [stats, setStats] = useState<Record<string, number> | null>(null);
  const [error, setError] = useState("");
  const pendingMetrics = useRef<AbortController | null>(null);
  useEffect(() => {
    if (!running) {
      pendingMetrics.current?.abort();
      setStats(null);
    }
    return () => pendingMetrics.current?.abort();
  }, [running]);
  async function browse(key: keyof PrefixSettings) {
    try {
      const kind =
        key === "model" || key === "cacheRoot"
          ? "models"
          : key === "runtimeDir"
            ? "root"
            : key === "evidence"
              ? "evidence"
              : "python";
      const path = await invoke<string | null>("select_path", { kind });
      if (path) onChange({ ...value, [key]: path });
    } catch (e) {
      setError(String(e));
    }
  }
  return (
    <section className="prefix-settings">
      <p className="knowledge-hint">
        MLX SSD · 512 / 128 · 1{" "}
        <span title={t.help} aria-label={t.help} tabIndex={0}>
          <Info size={14} />
        </span>
      </p>
      {(
        [
          "model",
          "python",
          "runtimeDir",
          "storeExecutable",
          "cacheRoot",
          "evidence",
        ] as const
      ).map((key) => (
        <label key={key}>
          {t[key]}
          <div className="path-input">
            <input
              value={value[key]}
              aria-label={t[key]}
              disabled={locked}
              spellCheck={false}
              onChange={(e) => onChange({ ...value, [key]: e.target.value })}
              onBlur={() => void onSave().catch((e) => setError(String(e)))}
            />
            {desktop && (
              <button
                type="button"
                className="icon-button"
                aria-label={t[key]}
                title={t[key]}
                disabled={locked}
                onClick={() => void browse(key)}
              >
                <FolderOpen />
              </button>
            )}
          </div>
        </label>
      ))}
      <label>
        {t.quotaMiB}
        <input
          type="number"
          min={64}
          max={4096}
          value={value.quotaMiB}
          disabled={locked}
          onChange={(e) =>
            onChange({ ...value, quotaMiB: Number(e.target.value) })
          }
          onBlur={() => void onSave().catch((e) => setError(String(e)))}
        />
      </label>
      <PrefixCapacityPanel language={language} locked={locked} configurationKey={`${value.cacheRoot}\0${value.quotaMiB}`} onSave={onSave} />
      <button
        type="button"
        disabled={!running}
        onClick={() => {
          setError("");
          pendingMetrics.current?.abort();
          const controller = new AbortController();
          pendingMetrics.current = controller;
          void request("/studio/prefix-cache", undefined, controller.signal)
            .then((r) => r.json())
            .then((value) => {
              if (!controller.signal.aborted) setStats(value);
            })
            .catch((e) => {
              if (!controller.signal.aborted) setError(String(e));
            });
        }}
      >
        <RefreshCw size={14} />
        {t.refresh}
      </button>
      {stats ? (
        <dl className="prefix-stats">
          {(["hits", "misses", "writes", "skippedWrites", "errors"] as const).map((k) => (
            <div key={k}>
              <dt>{t[k]}</dt>
              <dd>{stats[k] ?? "—"}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="knowledge-hint">{t.unavailable}</p>
      )}
      {error && (
        <p role="alert" className="knowledge-error">
          {error}
        </p>
      )}
    </section>
  );
}
