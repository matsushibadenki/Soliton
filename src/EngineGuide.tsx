import { useState } from "react";
import type { Language } from "./i18n";

export type Profile = {
  model: string;
  task: "text" | "media" | "voice";
  engine: string;
  setup: "strata" | "shards" | "mlx" | "ssd" | "media" | "voice";
  context: number | null;
  output: number | null;
  runtime: boolean;
  evidence: boolean | null;
  version: string | null;
};

const labels = {
  en: {
    mediaTask: "Media to text",
    evidenceFound: "Qualification file present · checked on use",
    evidenceMissing: "Qualification file needed",
    title: "Execution profiles",
    model: "Model",
    task: "Task",
    text: "Text chat",
    voice: "Realtime voice",
    none: "No reviewed profile for this model and task.",
    ctx: "Tested context",
    out: "Output",
    config: "See model configuration",
    ready: "Runtime paths found",
    missing: "Check runtime paths",
    strata: "Prepare local SafeTensors with bundled Strata.",
    shards:
      "Select a supported complete shard set and prepare the Metal config.",
    mlx: "Set the MLX model directory, Python, backend executable and matching qualification.",
    ssd: "Set the MLX worker, SSD store, cache directory and volume-bound qualification.",
    media:
      "Select the official GGUF, projector and matching media qualification.",
    audio:
      "Configure the GGUF, projector, media and Realtime qualifications. Allow microphone access for recording.",
  },
  ja: {
    mediaTask: "画像・音声・動画へのテキスト回答",
    evidenceFound: "資格ファイルあり · 利用時に照合",
    evidenceMissing: "資格ファイルが必要",
    title: "実行プロファイル",
    model: "モデル",
    task: "用途",
    text: "テキストチャット",
    voice: "リアルタイム音声",
    none: "このモデルと用途に対応する検証済みプロファイルはありません。",
    ctx: "検証済みコンテキスト",
    out: "出力",
    config: "モデル設定を参照",
    ready: "ランタイムのパスを確認済み",
    missing: "ランタイムのパスを要確認",
    strata: "同梱のStrataでローカルのSafeTensorsを準備してください。",
    shards: "対応する全シャードを選び、Metal設定を準備してください。",
    mlx: "MLXモデルのフォルダ、Python、バックエンド実行ファイル、一致する資格情報を指定してください。",
    ssd: "MLXワーカー、SSDストア、保存先、そのボリュームの資格情報を指定してください。",
    media:
      "公式GGUF、プロジェクター、一致するメディア資格情報を指定してください。",
    audio:
      "GGUF、プロジェクター、メディアとRealtimeの資格情報を設定してください。録音にはマイク権限が必要です。",
  },
  zh: {
    mediaTask: "图像、音频或视频转文本回复",
    evidenceFound: "已找到验证文件 · 使用时核验",
    evidenceMissing: "需要验证文件",
    title: "执行配置",
    model: "模型",
    task: "用途",
    text: "文本对话",
    voice: "实时语音",
    none: "此模型和用途暂无已验证配置。",
    ctx: "已验证上下文",
    out: "输出",
    config: "参见模型配置",
    ready: "已找到运行时路径",
    missing: "请检查运行时路径",
    strata: "使用内置Strata准备本地SafeTensors。",
    shards: "选择受支持的完整分片并准备Metal配置。",
    mlx: "设置MLX模型目录、Python、后端可执行文件及匹配的验证记录。",
    ssd: "设置MLX工作进程、SSD存储、缓存目录及对应卷的验证记录。",
    media: "选择官方GGUF、投影器及匹配的媒体验证记录。",
    audio: "配置GGUF、投影器、媒体和实时语音验证记录。录音需要麦克风权限。",
  },
};

export function EngineGuide({
  profiles,
  language,
  model,
}: {
  profiles: Profile[];
  language: Language;
  model?: string | null;
}) {
  const t = labels[language];
  const [choice, setChoice] = useState("");
  const [task, setTask] = useState<Profile["task"]>("text");
  const models = [...new Set(profiles.map((p) => p.model))];
  const selected = model === undefined ? choice || models[0] : model;
  const matches = profiles.filter(
    (p) => p.model === selected && p.task === task,
  );
  return (
    <section aria-label={t.title} className="engine-guide">
      <strong>{t.title}</strong>
      <div className="engine-guide-fields">
        {model === undefined && (
          <label>
            {t.model}
            <select
              value={selected || ""}
              onChange={(e) => setChoice(e.target.value)}
            >
              {models.map((name) => (
                <option key={name}>{name}</option>
              ))}
            </select>
          </label>
        )}
        <label>
          {t.task}
          <select
            value={task}
            onChange={(e) => setTask(e.target.value as Profile["task"])}
          >
            <option value="text">{t.text}</option>
            <option value="media">{t.mediaTask}</option>
            <option value="voice">{t.voice}</option>
          </select>
        </label>
      </div>
      {matches.map((p) => (
        <div key={p.engine} className="engine-guide-row">
          <b>
            {p.engine}
            {p.version && ` · ${p.version}`}
          </b>
          <span>{p.runtime ? t.ready : t.missing}</span>
          <span>
            {t.ctx} · {p.context?.toLocaleString() ?? t.config}
            {p.output && ` · ${t.out} ≤ ${p.output}`}
          </span>
          {p.evidence != null && (
            <span>{p.evidence ? t.evidenceFound : t.evidenceMissing}</span>
          )}
          <small>{t[p.setup === "voice" ? "audio" : p.setup]}</small>
        </div>
      ))}
      {!matches.length && <small>{t.none}</small>}
    </section>
  );
}
