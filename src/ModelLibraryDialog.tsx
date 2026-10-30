import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  CheckCircle2,
  Circle,
  FolderOpen,
  Info,
  Plus,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { request, desktop } from "./client";
import { Modal } from "./PreferencesDialog";
import { words, type Language } from "./i18n";
type Source = "strata" | "lmstudio" | "unsloth" | "ollama" | "custom";
type ModelFile = {
  name: string;
  path: string;
  selectionPath?: string;
  bytes: number;
  format: string;
  status:
    | "configurationRequired"
    | "unsupportedFormat"
    | "invalidFile"
    | "unreadable";
  sources: Source[];
};
type Scan = {
  roots: {
    source: Source;
    path: string;
    status: "scanned" | "missing" | "unreadable" | "limited";
    files: number;
    errors: number;
  }[];
  files: ModelFile[];
  limited: boolean;
};
const sources: Source[] = ["strata", "lmstudio", "unsloth", "ollama", "custom"];
export function ModelLibraryDialog({
  language,
  directories,
  locked,
  onDirectories,
  onUse,
  engineOffline,
  onClose,
}: {
  language: Language;
  directories: string[];
  locked: boolean;
  onDirectories: (paths: string[]) => Promise<void>;
  onUse: (path: string) => Promise<void>;
  engineOffline: boolean;
  onClose: () => void;
}) {
  const t = words[language];
  const [selection, setSelection] = useState<ModelFile | null>(null);
  const [check, setCheck] = useState<{
    kind: string;
    model?: string;
    workingGb?: number;
    freeBytes?: number | null;
    reason?: string;
  } | null>(null);
  const [checking, setChecking] = useState(false);
  const [applying, setApplying] = useState(false);
  const selectionController = useRef<AbortController | null>(null);
  async function selectModel(model: ModelFile) {
    selectionController.current?.abort();
    const abort = new AbortController();
    selectionController.current = abort;
    setSelection(model);
    setCheck(null);
    setChecking(true);
    try {
      const info = await request(
        "/studio/action",
        {
          action: "inspectLocalModel",
          path: model.selectionPath || model.path,
        },
        abort.signal,
      ).then((r) => r.json());
      if (!abort.signal.aborted) setCheck(info);
    } catch (e) {
      if (!abort.signal.aborted)
        setCheck({ kind: "unsupported", reason: (e as Error).message });
    } finally {
      if (!abort.signal.aborted) setChecking(false);
    }
  }
  const [scan, setScan] = useState<Scan>();
  const [busy, setBusy] = useState(true);
  const [failure, setFailure] = useState("");
  const [folder, setFolder] = useState("");
  const [downloadUrl, setDownloadUrl] = useState("");
  const [hubToken, setHubToken] = useState("");
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<{ received: number; total: number | null } | null>(null);
  useEffect(() => {
    if (!desktop) return;
    let dispose: (() => void) | undefined;
    let alive = true;
    void listen<{ received: number; total: number | null }>("model-download-progress", (event) => setProgress(event.payload)).then((unlisten) => { if (alive) dispose = unlisten; else unlisten(); });
    return () => { alive = false; dispose?.(); };
  }, []);
  const downloadText = {
    en: { cancel: "Stop", label: "Hugging Face URL", button: "Download", busy: "Downloading…", hint: "Repository or file URL. Submit the same URL to resume. Optional read token is not saved." },
    ja: { cancel: "停止", label: "Hugging Face URL", button: "ダウンロード", busy: "ダウンロード中…", hint: "リポジトリまたはファイルURL。同じURLで再開できます。任意の読み取りトークンは保存しません。" },
    zh: { cancel: "停止", label: "Hugging Face网址", button: "下载", busy: "正在下载…", hint: "输入仓库或文件网址。再次提交相同网址可继续下载。可选的读取令牌不会保存。" },
  }[language];
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<Source | "all">("all");
  const [limit, setLimit] = useState(50);
  const controller = useRef<AbortController | null>(null);
  const sourceNames = {
    strata: t.originalLocation,
    lmstudio: "LM Studio",
    unsloth: "Unsloth Studio",
    ollama: "Ollama",
    custom: t.customSource,
  };
  async function rescan(refresh = true) {
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    setBusy(true);
    setFailure("");
    try {
      const result: Scan = await request(
        "/studio/models" + (refresh ? "?refresh=true" : ""),
        undefined,
        abort.signal,
      ).then((r) => r.json());
      if (!abort.signal.aborted) setScan(result);
    } catch (e) {
      if (!abort.signal.aborted) setFailure((e as Error).message);
    } finally {
      if (!abort.signal.aborted) setBusy(false);
    }
  }
  useEffect(() => {
    rescan(false);
    return () => {
      controller.current?.abort();
      selectionController.current?.abort();
    };
  }, []);
  async function changeDirectories(next: string[]) {
    setBusy(true);
    setFailure("");
    try {
      await onDirectories(next);
      setFolder("");
      await rescan();
    } catch (e) {
      setFailure((e as Error).message);
      setBusy(false);
    }
  }
  const files =
    scan?.files.filter(
      (m) =>
        (source === "all" || m.sources.includes(source)) &&
        (m.name + " " + m.path).toLowerCase().includes(query.toLowerCase()),
    ) || [];
  function size(bytes: number) {
    const unit = bytes > 0 ? Math.min(4, Math.floor(Math.log10(bytes) / 3)) : 0;
    return (
      new Intl.NumberFormat(language, { maximumFractionDigits: 1 }).format(
        bytes / 1000 ** unit,
      ) +
      " " +
      ["B", "KB", "MB", "GB", "TB"][unit]
    );
  }
  const rootLabel = (status: Scan["roots"][number]["status"]) =>
    status === "missing"
      ? t.scanMissing
      : status === "limited"
        ? t.scanLimited
        : t[status];
  return (
    <Modal
      title={t.localModels}
      language={language}
      onClose={onClose}
      className="model-library-dialog"
    >
      <div className="model-library-body">
        <header className="model-library-toolbar">
          <span>{t.scanModels}</span>
          <span
            title={t.modelScanHelp}
            tabIndex={0}
            aria-label={t.modelScanHelp}
          >
            <Info size={15} />
          </span>
          <button
            className="icon-button"
            aria-label={t.refresh}
            title={t.refresh}
            disabled={busy}
            onClick={() => rescan()}
          >
            <RefreshCw className={busy ? "scan-spinning" : ""} />
          </button>
        </header>
        <details className="model-roots">
          <summary>
            {t.modelLocations} <span>{scan?.roots.length ?? "—"}</span>
          </summary>
          <div>
            {scan?.roots.map((root) => (
              <div className="model-root" key={root.source + root.path}>
                <span>{sourceNames[root.source]}</span>
                <code title={root.path}>{root.path}</code>
                <small
                  title={
                    root.errors ? `${root.errors} × ${t.unreadable}` : undefined
                  }
                  className={root.status === "scanned" ? "" : "muted"}
                >
                  {rootLabel(root.status)}
                  {root.errors > 0 ? " ⚠" : ""}
                </small>
              </div>
            ))}
          </div>
        </details>
        {desktop && <section className="custom-model-folders">
          <label htmlFor="hf-url">{downloadText.label} <span tabIndex={0} title={downloadText.hint} aria-label={downloadText.hint}><Info size={14} /></span></label>
          <form className="model-folder-form" onSubmit={async (e) => {
            e.preventDefault();
            setDownloading(true);
            setProgress(null);
            setFailure("");
            try {
              const path = await invoke<string>("download_huggingface_file", { url: downloadUrl.trim(), token: hubToken });
              setDownloadUrl("");
              if (!directories.includes(path)) await changeDirectories([...directories, path]);
              else await rescan();
            } catch (error) { setFailure(String(error)); }
            finally { setHubToken(""); setDownloading(false); }
          }}>
            <input id="hf-url" type="url" value={downloadUrl} onChange={(e) => setDownloadUrl(e.target.value)} placeholder="https://huggingface.co/…/resolve/main/…" disabled={downloading || locked} />
            <input type="password" autoComplete="off" aria-label="Hugging Face token" placeholder="HF token" value={hubToken} disabled={downloading} onChange={(e) => setHubToken(e.target.value)} />
            <button disabled={downloading || locked || !downloadUrl.trim() || directories.length >= 32}>{downloading ? downloadText.busy : downloadText.button}</button>
          </form>
          {downloading && <div className="hub-download-progress">
            <button type="button" onClick={() => void invoke("cancel_model_download").catch((e) => setFailure(String(e)))}>{downloadText.cancel}</button>
            <progress aria-label={downloadText.busy} value={progress?.total ? progress.received : undefined} max={progress?.total || undefined} />
            {progress?.total && <output>{Math.min(100, Math.floor(progress.received / progress.total * 100))}%</output>}
          </div>}
        </section>}
        <section className="custom-model-folders">
          <label htmlFor="model-folder">{t.customLocations}</label>
          <form
            className="model-folder-form"
            onSubmit={(e) => {
              e.preventDefault();
              const path = folder.trim();
              if (path && !directories.includes(path))
                changeDirectories([...directories, path]);
            }}
          >
            <input
              id="model-folder"
              value={folder}
              disabled={busy || locked || directories.length >= 32}
              placeholder={t.modelFolderPlaceholder}
              onChange={(e) => setFolder(e.target.value)}
            />
            {desktop && (
              <button
                type="button"
                className="icon-button"
                aria-label={t.browse}
                title={t.browse}
                disabled={busy || locked || directories.length >= 32}
                onClick={async () => {
                  try {
                    const path = await invoke<string | null>("select_path", {
                      kind: "models",
                    });
                    if (path && !directories.includes(path))
                      await changeDirectories([...directories, path]);
                  } catch (e) {
                    setFailure((e as Error).message);
                  }
                }}
              >
                <FolderOpen />
              </button>
            )}
            <button
              className="icon-button"
              aria-label={t.addLocation}
              title={t.addLocation}
              disabled={
                busy ||
                locked ||
                !folder.trim() ||
                directories.includes(folder.trim()) ||
                directories.length >= 32
              }
            >
              <Plus />
            </button>
          </form>
          {directories.length > 0 && (
            <ul className="custom-folder-list">
              {directories.map((path) => (
                <li key={path}>
                  <code title={path}>{path}</code>
                  <button
                    className="icon-button"
                    aria-label={`${t.removeLocation} · ${path}`}
                    title={t.removeLocation}
                    disabled={busy || locked}
                    onClick={() =>
                      changeDirectories(directories.filter((p) => p !== path))
                    }
                  >
                    <X />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
        <nav className="model-source-filters" aria-label={t.modelLocations}>
          {(["all", ...sources] as const).map((id) => (
            <button
              key={id}
              aria-pressed={source === id}
              onClick={() => {
                setSource(id);
                setLimit(50);
              }}
            >
              {id === "all" ? t.allSources : sourceNames[id]}
              <span>
                {scan?.files.filter(
                  (m) => id === "all" || m.sources.includes(id),
                ).length ?? "—"}
              </span>
            </button>
          ))}
        </nav>
        <label className="model-search">
          <Search size={15} />
          <input
            aria-label={t.modelSearch}
            placeholder={t.modelSearch}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setLimit(50);
            }}
          />
        </label>
        {failure && (
          <p className="notice error" role="alert">
            {t.scanFailed}: {failure}
          </p>
        )}
        {scan?.limited && (
          <p className="notice" role="status">
            {t.scanLimited}
          </p>
        )}
        <div className="model-inventory" aria-busy={busy}>
          {busy && (
            <p className="inventory-empty" role="status">
              {t.scanningModels}
            </p>
          )}
          {!busy && !files.length && !failure && (
            <p className="inventory-empty">{t.noModels}</p>
          )}
          {files.slice(0, limit).map((m) => (
            <article
              className={
                "model-file " +
                (selection?.path === m.path ? "model-selected" : "")
              }
              key={m.path}
            >
              <div className="model-file-heading">
                <button
                  className="icon-button model-select"
                  aria-label={`${t.selectModel} · ${m.name}`}
                  aria-pressed={selection?.path === m.path}
                  disabled={
                    busy ||
                    applying ||
                    m.status === "invalidFile" ||
                    m.status === "unreadable" ||
                    (m.format !== "GGUF" && m.format !== "Strata pack")
                  }
                  title={t.selectModel}
                  onClick={() => selectModel(m)}
                >
                  {selection?.path === m.path ? <CheckCircle2 /> : <Circle />}
                </button>
                <strong title={m.name}>{m.name}</strong>
                <code>{m.format}</code>
                <span>{size(m.bytes)}</span>
              </div>
              <code className="model-file-path" title={m.path}>
                {m.path}
              </code>
              <div className="model-file-meta">
                <span>
                  {m.sources.map((id) => sourceNames[id]).join(" · ")}
                </span>
                <small
                  className={
                    m.status === "invalidFile" || m.status === "unreadable"
                      ? "check-missing"
                      : ""
                  }
                >
                  {t[m.status]}
                </small>
              </div>
            </article>
          ))}
          {files.length > limit && (
            <button className="show-more" onClick={() => setLimit(limit + 50)}>
              {t.showMore} ({files.length - limit})
            </button>
          )}
        </div>
      </div>
      {selection && (
        <footer className="model-use-footer">
          <div>
            <strong>{check?.model || selection.name}</strong>
            <small role="status">
              {checking
                ? t.checkingModel
                : check?.kind === "configured"
                  ? t.configuredModel
                  : check?.kind === "prepare"
                    ? `${t.localReuse} · ${check.workingGb?.toFixed(1)} GB ${t.workspace}`
                    : t.unsupportedLocal}
            </small>
            {check?.reason && <p role="alert">{check.reason}</p>}
            {!engineOffline && <small>{t.stopForSetup}</small>}
            {check?.kind === "prepare" &&
              check.freeBytes != null &&
              check.freeBytes / 1e9 < (check.workingGb || 0) && (
                <small>{t.diskShort}</small>
              )}
          </div>
          <span
            title={t.localReuseHelp}
            aria-label={t.localReuseHelp}
            tabIndex={0}
          >
            <Info size={15} />
          </span>
          <button
            className="primary"
            disabled={
              busy ||
              locked ||
              !engineOffline ||
              checking ||
              applying ||
              !check ||
              check.kind === "unsupported" ||
              (check.kind === "prepare" &&
                check.freeBytes != null &&
                check.freeBytes / 1e9 < (check.workingGb || 0))
            }
            onClick={async () => {
              setApplying(true);
              setFailure("");
              try {
                await onUse(selection.selectionPath || selection.path);
              } catch (e) {
                setFailure((e as Error).message);
              } finally {
                setApplying(false);
              }
            }}
          >
            {applying ? t.setupRunning : t.useModel}
          </button>
        </footer>
      )}
    </Modal>
  );
}
