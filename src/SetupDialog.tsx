import { useEffect, useRef, useState } from "react";
import {
  CheckCircle2,
  AlertCircle,
  Download,
  RefreshCw,
  Square,
  X,
  Info,
  FolderSearch,
} from "lucide-react";
import { words, type Language } from "./i18n";
export type ConfigInfo = {
  path: string;
  model: string;
  valid: boolean;
  engineExists: boolean;
  tokenizerExists: boolean;
  error: string | null;
  backend: string;
};
export type Inspection = {
  rootExists: boolean;
  setupAvailable: boolean;
  supportedHost: boolean;
  python: { supported: boolean; dependencies: boolean; version?: string };
  configs: ConfigInfo[];
  selected: ConfigInfo | null;
  models: { id: string; downloadGb: number; workingGb: number }[];
  freeBytes: number | null;
  recommended: string;
};
export type Preparation = {
  status: string;
  model: string;
  error: string | null;
  localReuse?: boolean;
};
export function SetupDialog({
  language,
  localView = false,
  inspection,
  job,
  engineOffline,
  config,
  onInspect,
  onModels,
  onConfig,
  onPrepare,
  onCancel,
  onClose,
}: {
  language: Language;
  localView?: boolean;
  inspection: Inspection | null;
  job: Preparation;
  engineOffline: boolean;
  config: string;
  onInspect: () => Promise<void>;
  onModels: () => void;
  onConfig: (path: string) => Promise<void>;
  onPrepare: (model: string) => Promise<void>;
  onCancel: () => Promise<void>;
  onClose: () => void;
}) {
  const t = words[language];
  const ref = useRef<HTMLDialogElement>(null);
  const [model, setModel] = useState(
    (["running", "cancelling"].includes(job.status) && job.model) ||
      inspection?.recommended ||
      "Q2_0",
  );
  const [consent, setConsent] = useState(false);
  const [working, setWorking] = useState(false);
  const [failure, setFailure] = useState("");
  const busy = ["running", "cancelling"].includes(job.status);
  const reuseOnly = localView || (busy && !!job.localReuse);
  const selected = inspection?.models.find((m) => m.id === model);
  const workingGb = selected
    ? selected.workingGb - (reuseOnly ? selected.downloadGb : 0)
    : undefined;
  const free =
    inspection?.freeBytes == null ? null : inspection.freeBytes / 1e9;
  useEffect(() => {
    ref.current?.showModal();
    return () => ref.current?.close();
  }, []);
  async function run(fn: () => Promise<void>) {
    setFailure("");
    setWorking(true);
    try {
      await fn();
    } catch (e) {
      setFailure((e as Error).message);
    } finally {
      setWorking(false);
    }
  }
  useEffect(() => {
    if (["running", "cancelling"].includes(job.status) && job.model) {
      setModel(job.model);
      setConsent(false);
    }
  }, [job.status, job.model]);
  useEffect(() => {
    if (job.status === "completed") {
      onInspect().catch((e: Error) => setFailure(e.message));
    }
  }, [job.status]);
  const checks: [string, boolean, string][] = [
    [t.root, !!inspection?.rootExists, ""],
    [
      t.python,
      !!inspection?.python.supported,
      inspection?.python.version || "—",
    ],
    [t.dependencies, !!inspection?.python.dependencies, ""],
    [t.config, !!inspection?.selected?.valid, ""],
    [t.engineFile, !!inspection?.selected?.engineExists, ""],
    [t.tokenizer, !!inspection?.selected?.tokenizerExists, ""],
  ];
  return (
    <dialog
      className="setup-dialog"
      aria-labelledby="setup-title"
      ref={ref}
      onCancel={onClose}
      onClose={onClose}
    >
      <header className="panel-header">
        <h2 id="setup-title">
          <Download size={17} />
          {t.prepare}
        </h2>
        <div className="tools">
          <button
            className="icon-button"
            aria-label={t.localModels}
            title={t.localModels}
            onClick={onModels}
          >
            <FolderSearch />
          </button>
          <button
            className="icon-button"
            aria-label={t.refresh}
            title={t.refresh}
            disabled={working || busy}
            onClick={() => run(onInspect)}
          >
            <RefreshCw />
          </button>
          <button
            className="icon-button"
            aria-label={t.close}
            title={t.close}
            onClick={onClose}
          >
            <X />
          </button>
        </div>
      </header>
      <div className="setup-body">
        <div className="diagnostics" aria-label={t.readiness}>
          {checks.map(([label, ok, detail]) => (
            <div className="diagnostic" key={label}>
              {ok ? (
                <CheckCircle2 className="check-ok" size={15} />
              ) : (
                <AlertCircle className="check-missing" size={15} />
              )}
              <span>{label}</span>
              <code>{detail || (ok ? t.available : t.missing)}</code>
            </div>
          ))}
        </div>
        <label>
          {t.installedConfigs}
          <select
            aria-label={t.installedConfigs}
            value={config}
            disabled={working || busy || !engineOffline}
            onChange={(e) => run(() => onConfig(e.target.value))}
          >
            <option value="">{t.selectConfig}</option>
            {config && !inspection?.configs.some((c) => c.path === config) ? (
              <option value={config}>
                {inspection?.selected?.model || config.split("/").pop()}
              </option>
            ) : null}
            {inspection?.configs.map((c) => (
              <option key={c.path} value={c.path}>
                {c.model || c.path.split("/").pop()}
                {!c.valid ? " ⚠" : ""}
              </option>
            ))}
          </select>
        </label>
        <div className="divider" />
        <div className="setup-model-row">
          <label>
            {t.model}
            <select
              aria-label={t.model}
              value={model}
              disabled={busy || working || reuseOnly}
              onChange={(e) => {
                setModel(e.target.value);
                setConsent(false);
              }}
            >
              {inspection?.models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id}
                </option>
              ))}
            </select>
          </label>
          <div className="download-size">
            <code>
              {reuseOnly ? "0" : selected?.downloadGb.toFixed(1) || "—"} GB
            </code>
            <span>{t.downloadSize}</span>
          </div>
        </div>
        <div className="value-row">
          <span>{t.freeDisk}</span>
          <code>{free?.toFixed(1) ?? "—"} GB</code>
        </div>
        <div className="value-row">
          <span>
            {t.workspace}
            <span title={t.workspaceHelp}>
              <Info size={13} />
            </span>
          </span>
          <code>{workingGb?.toFixed(1) || "—"} GB</code>
        </div>
        {free !== null && selected && free < (workingGb || 0) ? (
          <p className="setup-warning" role="status">
            {t.diskShort}
          </p>
        ) : null}
        {!inspection?.supportedHost ? (
          <p className="setup-warning">{t.macOnly}</p>
        ) : null}
        {!engineOffline ? (
          <p className="setup-warning">{t.stopForSetup}</p>
        ) : null}
        {busy ? (
          <div className="setup-progress" role="status">
            <Download size={16} />
            <span>
              {
                t[
                  job.status === "cancelling"
                    ? "setupCancelling"
                    : "setupRunning"
                ]
              }
            </span>
            <code>{job.model}</code>
          </div>
        ) : reuseOnly ? (
          <p className="check-ok" title={t.localReuseHelp}>
            {t.localReuse}
          </p>
        ) : (
          <label className="download-consent">
            <input
              type="checkbox"
              checked={consent}
              disabled={working}
              onChange={(e) => setConsent(e.target.checked)}
            />
            {t.downloadConsent.replace(
              "{size}",
              selected?.downloadGb.toFixed(1) || "—",
            )}
          </label>
        )}
        {job.status === "completed" ? (
          <p className="check-ok" role="status">
            {t.setupComplete}
          </p>
        ) : null}
        {job.status === "cancelled" ? (
          <p role="status">{t.setupCancelled}</p>
        ) : null}
        {failure || job.error ? (
          <p className="setup-warning" role="alert">
            {failure || job.error}
          </p>
        ) : null}
        <div className="setup-actions">
          {busy ? (
            <button
              disabled={job.status === "cancelling" || working}
              onClick={() => run(onCancel)}
            >
              <Square size={14} />
              {t.cancelPreparation}
            </button>
          ) : reuseOnly ? (
            <button onClick={onModels}>{t.localModels}</button>
          ) : (
            <button
              className="primary"
              disabled={
                !consent ||
                working ||
                !inspection?.supportedHost ||
                !inspection?.rootExists ||
                !inspection?.setupAvailable ||
                !inspection?.python.supported ||
                !engineOffline ||
                !selected ||
                (free !== null && free < selected.workingGb)
              }
              onClick={() =>
                run(async () => {
                  await onPrepare(model);
                  setConsent(false);
                })
              }
            >
              <Download size={15} />
              {t.prepare}
            </button>
          )}
        </div>
      </div>
    </dialog>
  );
}
