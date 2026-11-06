import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  BookOpen,
  FlaskConical,
  History,
  Info,
  FolderOpen,
  Plus,
  Trash2,
  Save,
  Play,
  Square,
  RefreshCw,
  Download,
  Check,
  Copy,
  Archive,
  LoaderCircle,
} from "lucide-react";
import { desktop, request } from "./client";
import type { Language } from "./i18n";
import { KnowledgePlanner, type GeneratedPlan } from "./KnowledgePlanner";
import { knowledgeWords } from "./knowledge-i18n";
import type { EvaluationCase, EvaluationKind } from "./knowledge-types";

type Category = EvaluationKind;
type Draft = {
  name: string;
  model: string;
  python: string;
  source: string;
  operation: "add" | "suppress";
  training: { prompt: string; response: string }[];
  evaluation: EvaluationCase[];
  steps: number;
  layers: number;
  learningRate: number;
  maxTokens: number;
};
type Answer = {
  text: string;
  tokens: number;
  elapsedMs: number;
  truncated: boolean;
  passed: boolean;
  reasons?: string[];
};
type Status = keyof typeof knowledgeWords.en.status;
type Experiment = {
  assessmentVersion?: number;
  id: string;
  createdAt: number;
  draft: Draft;
  status: Status;
  directory: string;
  results: { baseline: Answer | null; candidate: Answer | null }[];
  logs: string[];
  error: string | null;
  provenance: {
    sourceSha256: string;
    adapterSha256: string;
    versions: Record<string, string>;
  } | null;
  progress?: { iteration: number; loss: number; peakMemoryGb: number } | null;
};
type LabState = {
  workspace: {
    plan: GeneratedPlan | null;
    revision: number;
    view: string;
    draft: Draft;
    selected: string | null;
    experiments: Experiment[];
  };
  busy: boolean;
  suggestedModel: string;
  suggestedPython: string;
};
const terminal = new Set<Status>([
  "passed",
  "rejected",
  "failed",
  "cancelled",
  "interrupted",
  "retained",
  "archived",
]);

export function KnowledgeLab({
  language,
  active,
  online,
  engineOffline,
}: {
  language: Language;
  active: boolean;
  online: boolean;
  engineOffline: boolean;
}) {
  const t = knowledgeWords[language];
  const [state, setState] = useState<LabState>();
  const [draft, setDraft] = useState<Draft>();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [unsaved, setUnsaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const dirty = useRef(false),
    revision = useRef(0),
    sequence = useRef(0),
    commanding = useRef(false);
  const form = useRef<HTMLFormElement>(null);

  async function refresh(force = false, signal?: AbortSignal) {
    const seq = ++sequence.current;
    const next: LabState = await request(
      "/studio/knowledge",
      undefined,
      signal,
    ).then((r) => r.json());
    if (signal?.aborted || seq !== sequence.current) return;
    setState(next);
    if (!dirty.current || force) {
      revision.current = next.workspace.revision;
      setDraft({
        ...next.workspace.draft,
        model: next.workspace.draft.model || next.suggestedModel,
        python: next.workspace.draft.python || next.suggestedPython,
      });
      dirty.current = false;
      setUnsaved(false);
    }
  }
  useEffect(() => {
    if (!online || !active) return;
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        if (!commanding.current) await refresh(false, abort.signal);
      } catch (e) {
        if (!abort.signal.aborted) setError(String(e));
      }
      if (!abort.signal.aborted) timer = setTimeout(poll, 1200);
    }
    void poll();
    return () => {
      abort.abort();
      clearTimeout(timer);
    };
  }, [active, online]);
  function change(patch: Partial<Draft>) {
    dirty.current = true;
    setUnsaved(true);
    setDraft((d) => d && { ...d, ...patch });
  }
  async function command(action: string, data: Record<string, unknown> = {}) {
    commanding.current = true;
    setPending(true);
    setError("");
    try {
      ++sequence.current;
      await request("/studio/knowledge", { action, ...data });
      await refresh(
        action === "save" || action === "run" || action === "importPlan",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      commanding.current = false;
      setPending(false);
    }
  }
  async function browse(field: "model" | "python") {
    try {
      const path = await invoke<string | null>("select_path", {
        kind: field === "model" ? "models" : "python",
      });
      if (path) change({ [field]: path });
    } catch (e) {
      setError(String(e));
    }
  }
  async function reload() {
    commanding.current = true;
    setPending(true);
    setError("");
    try {
      await refresh(true);
    } catch (e) {
      setError(String(e));
    } finally {
      commanding.current = false;
      setPending(false);
    }
  }
  const locked = pending || !!state?.busy || !online;
  const view = state?.workspace.view || "edit";
  const experiments = state?.workspace.experiments || [];
  const selected =
    experiments.find((e) => e.id === state?.workspace.selected) ||
    experiments[0];
  const conflict = unsaved && state?.workspace.revision !== revision.current;
  const info = (label: string) => (
    <span
      className="knowledge-info"
      tabIndex={0}
      role="img"
      aria-label={label}
      title={label}
    >
      <Info size={14} />
    </span>
  );
  function exportReport(e: Experiment) {
    if (desktop) {
      void invoke("export_knowledge_report", { id: e.id }).catch((err) =>
        setError(String(err)),
      );
      return;
    }
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(e, null, 2)], { type: "application/json" }),
    );
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `knowledge-${e.id}.json`;
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  function answer(a: Answer | null) {
    return a ? (
      <>
        <div
          className={"knowledge-answer-status " + (a.passed ? "pass" : "fail")}
        >
          {a.passed ? <Check size={14} /> : <Square size={12} />}
          {a.passed ? t.pass : t.fail}
          <span>
            {a.tokens} tok · {(a.elapsedMs / 1000).toFixed(2)} s
          </span>
        </div>
        <p>{a.text || "—"}</p>
        {a.truncated && <small>{t.truncated}</small>}
        {!!a.reasons?.length && <ul className="knowledge-reasons">{a.reasons.map((reason) => <li key={reason}>{(t.reasons as Record<string,string>)[reason] || reason}</li>)}</ul>}
      </>
    ) : (
      <span className="muted">{t.waiting}</span>
    );
  }
  return (
    <section className="knowledge-lab" aria-label={t.knowledge}>
      <header className="knowledge-toolbar">
        <div
          className="knowledge-subtabs"
          role="tablist"
          aria-label={t.knowledge}
        >
          {(
            [
              ["edit", BookOpen],
              ["results", FlaskConical],
              ["history", History],
            ] as const
          ).map(([id, Icon], index, all) => (
            <button
              key={id}
              id={`knowledge-tab-${id}`}
              role="tab"
              aria-selected={view === id}
              aria-controls={`knowledge-${id}`}
              tabIndex={view === id ? 0 : -1}
              disabled={pending || !online}
              onClick={() => void command("view", { view: id })}
              onKeyDown={(e) => {
                if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key))
                  return;
                e.preventDefault();
                const next =
                  e.key === "Home"
                    ? 0
                    : e.key === "End"
                      ? 2
                      : (index + (e.key === "ArrowLeft" ? 2 : 1)) % 3;
                void command("view", { view: all[next][0] }).then(() =>
                  document
                    .getElementById(`knowledge-tab-${all[next][0]}`)
                    ?.focus(),
                );
              }}
            >
              <Icon size={15} />
              {t[id]}
            </button>
          ))}
        </div>
        <span className="knowledge-method">
          {t.method}
          {info(t.methodHelp)}
        </span>
      </header>
      {error && (
        <div className="knowledge-error" role="alert">
          <strong>{t.invalid}</strong>
          <span>{error}</span>
        </div>
      )}
      {!draft || !state ? (
        <div className="knowledge-empty" role="status">
          {t.loading}
        </div>
      ) : (
        <>
          <div
            id="knowledge-edit"
            role="tabpanel"
            aria-labelledby="knowledge-tab-edit"
            hidden={view !== "edit"}
            className="knowledge-page"
          >
            <form
              ref={form}
              onSubmit={(e) => {
                e.preventDefault();
                void command("run", { draft, revision: revision.current });
              }}
            >
              <KnowledgePlanner
                language={language}
                operation={draft.operation}
                revision={revision.current}
                plan={state.workspace.plan}
                disabled={locked}
                dirty={unsaved}
                onRefresh={() => refresh()}
                onImport={(id) =>
                  command("importPlan", { id, revision: revision.current })
                }
              />
              <div className="knowledge-actions">
                <span className="muted">{unsaved ? t.unsaved : t.saved}</span>
                <button
                  type="button"
                  className="icon-button"
                  title={t.reload}
                  aria-label={t.reload}
                  disabled={locked}
                  onClick={() => void reload()}
                >
                  <RefreshCw />
                </button>
                <button
                  type="button"
                  disabled={locked || conflict}
                  onClick={() =>
                    void command("save", { draft, revision: revision.current })
                  }
                >
                  <Save size={14} />
                  {t.save}
                </button>
                <button
                  type="submit"
                  className="primary"
                  disabled={locked || conflict || !engineOffline}
                  title={!engineOffline ? t.offline : t.run}
                >
                  <Play size={14} />
                  {t.run}
                </button>
              </div>
              {conflict && (
                <p role="alert" className="knowledge-error">
                  {t.draftConflict}
                </p>
              )}
              {!engineOffline && <p className="knowledge-hint">{t.offline}</p>}
              <fieldset disabled={locked}>
                <div className="knowledge-identity">
                  <label>
                    {t.name}
                    <input
                      required
                      maxLength={160}
                      value={draft.name}
                      onChange={(e) => change({ name: e.target.value })}
                    />
                  </label>
                  <div
                    className="knowledge-operation"
                    role="group"
                    aria-label={t.edit}
                  >
                    {(["add", "suppress"] as const).map((op) => (
                      <button
                        type="button"
                        key={op}
                        aria-pressed={draft.operation === op}
                        onClick={() => change({ operation: op })}
                      >
                        {t[op]}
                      </button>
                    ))}
                  </div>
                </div>
                {draft.operation === "suppress" && (
                  <p className="knowledge-hint">{t.suppression}</p>
                )}
                <div className="knowledge-paths">
                  {(["model", "python"] as const).map((field) => (
                    <label key={field}>
                      {t[field]}
                      <div className="knowledge-path-input">
                        <input
                          required
                          value={draft[field]}
                          spellCheck={false}
                          onChange={(e) => change({ [field]: e.target.value })}
                        />
                        {desktop && (
                          <button
                            type="button"
                            className="icon-button"
                            aria-label={`${t.browse}: ${t[field]}`}
                            title={t.browse}
                            onClick={() => void browse(field)}
                          >
                            <FolderOpen />
                          </button>
                        )}
                      </div>
                    </label>
                  ))}
                </div>
                <label>
                  {t.source}
                  {info(t.sourceHint)}
                  <input
                    required
                    value={draft.source}
                    maxLength={4096}
                    onChange={(e) => change({ source: e.target.value })}
                  />
                </label>
                <section className="knowledge-section">
                  <header>
                    <h2>
                      {t.training} <span>{draft.training.length}</span>
                    </h2>
                    {info(t.trainHelp)}
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t.addRow}
                      title={t.addRow}
                      disabled={draft.training.length >= 64}
                      onClick={() =>
                        change({
                          training: [
                            ...draft.training,
                            { prompt: "", response: "" },
                          ],
                        })
                      }
                    >
                      <Plus />
                    </button>
                  </header>
                  {draft.training.map((row, index) => (
                    <div className="knowledge-training-row" key={index}>
                      <label>
                        {t.question} {index + 1}
                        <textarea
                          required
                          rows={2}
                          maxLength={4096}
                          value={row.prompt}
                          onChange={(e) =>
                            change({
                              training: draft.training.map((r, i) =>
                                i === index
                                  ? { ...r, prompt: e.target.value }
                                  : r,
                              ),
                            })
                          }
                        />
                      </label>
                      <label>
                        {t.response} {index + 1}
                        <textarea
                          required
                          rows={2}
                          maxLength={4096}
                          value={row.response}
                          onChange={(e) =>
                            change({
                              training: draft.training.map((r, i) =>
                                i === index
                                  ? { ...r, response: e.target.value }
                                  : r,
                              ),
                            })
                          }
                        />
                      </label>
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={`${t.remove}: ${t.training} ${index + 1}`}
                        title={t.remove}
                        disabled={draft.training.length === 1}
                        onClick={() =>
                          change({
                            training: draft.training.filter(
                              (_, i) => i !== index,
                            ),
                          })
                        }
                      >
                        <Trash2 />
                      </button>
                    </div>
                  ))}
                </section>
                <section className="knowledge-section">
                  <header>
                    <h2>
                      {t.tests} <span>{draft.evaluation.length}</span>
                    </h2>
                    {info(t.testsHelp)}
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t.addTest}
                      title={t.addTest}
                      disabled={draft.evaluation.length >= 32}
                      onClick={() =>
                        change({
                          evaluation: [
                            ...draft.evaluation,
                            {
                              kind: "paraphrase",
                              prompt: "",
                              expected: "",
                              forbidden: "",
                              matchMode: "exact",
                              aliases: [],
                              language,
                            },
                          ],
                        })
                      }
                    >
                      <Plus />
                    </button>
                  </header>
                  {draft.evaluation.map((row, index) => {
                    const update = (patch: Partial<typeof row>) =>
                      change({
                        evaluation: draft.evaluation.map((r, i) =>
                          i === index ? { ...r, ...patch } : r,
                        ),
                      });
                    return (
                      <div className="knowledge-test-row" key={index}>
                        <div className="knowledge-case-meta"><label className="knowledge-category">
                          {index + 1}
                          <select
                            aria-label={`${t.tests} ${index + 1}`}
                            value={row.kind}
                            onChange={(e) =>
                              update({ kind: e.target.value as Category })
                            }
                          >
                            {(
                              ["target", "paraphrase", "indirect", "locality"] as const
                            ).map((k) => (
                              <option key={k} value={k}>
                                {t[k]}
                              </option>
                            ))}
                          </select>
                        </label><label>{t.language}<select aria-label={`${t.language} ${index+1}`} value={row.language || "und"} onChange={e=>update({language:e.target.value as EvaluationCase["language"]})}>
                          <option value="und">—</option><option value="en">English</option><option value="ja">日本語</option><option value="zh">简体中文</option>
                        </select></label><label>{t.matchMode}{info(t.matchHelp)}<select aria-label={`${t.matchMode} ${index+1}`} value={row.matchMode || "contains"} onChange={e=>update({matchMode:e.target.value as EvaluationCase["matchMode"]})}>
                          <option value="exact">{t.exact}</option><option value="contains">{t.contains}</option>
                        </select></label></div>
                        <label>
                          {t.question}
                          <textarea
                            required
                            rows={2}
                            maxLength={4096}
                            value={row.prompt}
                            onChange={(e) => update({ prompt: e.target.value })}
                          />
                        </label>
                        <div className="knowledge-phrases">
                          <label>
                            {t.expected}
                            <input
                              required
                              maxLength={4096}
                              value={row.expected}
                              onChange={(e) =>
                                update({ expected: e.target.value })
                              }
                            />
                          </label>
                          <label>{t.aliases}{info(t.aliasesHelp)}<textarea rows={2} maxLength={32768} aria-label={`${t.aliases} ${index+1}`} value={(row.aliases || []).join("\n")}
                            onChange={e=>update({aliases:e.target.value ? e.target.value.split("\n") : []})}
                            onBlur={()=>{const aliases=(row.aliases || []).map(s=>s.trim()).filter(Boolean);if(JSON.stringify(aliases)!==JSON.stringify(row.aliases || [])) update({aliases});}} /></label>
                          <label>
                            {t.forbidden}
                            <input
                              required={
                                draft.operation === "suppress" &&
                                row.kind !== "locality"
                              }
                              maxLength={4096}
                              value={row.forbidden}
                              onChange={(e) =>
                                update({ forbidden: e.target.value })
                              }
                            />
                          </label>
                        </div>
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={`${t.remove}: ${t.tests} ${index + 1}`}
                          title={t.remove}
                          disabled={draft.evaluation.length <= 4}
                          onClick={() =>
                            change({
                              evaluation: draft.evaluation.filter(
                                (_, i) => i !== index,
                              ),
                            })
                          }
                        >
                          <Trash2 />
                        </button>
                      </div>
                    );
                  })}
                </section>
                <details className="knowledge-settings">
                  <summary>{t.settings}</summary>
                  <div>
                    {(
                      [
                        ["steps", 1, 500, 1],
                        ["layers", 1, 32, 1],
                        ["learningRate", 0.000001, 0.001, 0.000001],
                        ["maxTokens", 16, 512, 1],
                      ] as const
                    ).map(([key, min, max, step]) => (
                      <label key={key}>
                        {
                          t[
                            key === "learningRate"
                              ? "rate"
                              : key === "maxTokens"
                                ? "tokens"
                                : key
                          ]
                        }
                        <input
                          type="number"
                          required
                          min={min}
                          max={max}
                          step={step}
                          value={draft[key]}
                          onChange={(e) =>
                            change({
                              [key]:
                                e.target.value === ""
                                  ? 0
                                  : Number(e.target.value),
                            })
                          }
                        />
                      </label>
                    ))}
                  </div>
                </details>
              </fieldset>
            </form>
          </div>
          <div
            id="knowledge-results"
            role="tabpanel"
            aria-labelledby="knowledge-tab-results"
            hidden={view !== "results"}
            className="knowledge-page"
          >
            {!selected ? (
              <div className="knowledge-empty">
                <FlaskConical />
                <h2>{t.empty}</h2>
                <p>{t.emptyHelp}</p>
              </div>
            ) : (
              <>
                <header className="knowledge-result-header">
                  <div>
                    <h1>{selected.draft.name}</h1>
                    <span className="muted">
                      {selected.draft.operation === "suppress"
                        ? t.suppression
                        : t.add}
                    </span>
                  </div>
                  <span
                    className={`knowledge-status status-${selected.status}`}
                    role="status"
                  >
                    {!terminal.has(selected.status) && (
                      <LoaderCircle className="spinning" size={14} />
                    )}
                    {t.status[selected.status]}
                  </span>
                </header>
                <div className="knowledge-actions">
                  {state.busy && !terminal.has(selected.status) && (
                    <button
                      disabled={pending}
                      onClick={() => void command("cancel")}
                    >
                      <Square size={14} />
                      {t.cancel}
                    </button>
                  )}
                  <button
                    disabled={
                      pending || state.busy || selected.status !== "passed" || selected.assessmentVersion !== 1
                    }
                    title={t.gates}
                    onClick={() => void command("retain", { id: selected.id })}
                  >
                    <Check size={14} />
                    {t.retain}
                  </button>
                  <button
                    className="icon-button"
                    title={t.export}
                    aria-label={t.export}
                    onClick={() => exportReport(selected)}
                  >
                    <Download />
                  </button>
                  <button
                    className="icon-button"
                    title={t.archive}
                    aria-label={t.archive}
                    disabled={
                      pending ||
                      state.busy ||
                      !terminal.has(selected.status) ||
                      selected.status === "archived"
                    }
                    onClick={() => void command("archive", { id: selected.id })}
                  >
                    <Archive />
                  </button>
                </div>
                {selected.error && (
                  <p className="knowledge-error" role="alert">
                    {selected.error}
                  </p>
                )}
                {selected.progress && (
                  <div className="knowledge-progress">
                    <progress
                      value={selected.progress.iteration}
                      max={selected.draft.steps}
                      aria-label={t.steps}
                    />
                    <span>
                      {selected.progress.iteration} / {selected.draft.steps} ·{" "}
                      {t.loss} {selected.progress.loss.toFixed(3)} ·{" "}
                      {selected.progress.peakMemoryGb.toFixed(2)} GB
                    </span>
                  </div>
                )}
                <p className="knowledge-hint">
                  {selected.assessmentVersion === 1 ? t.assessment : t.legacyAssessment}
                  {info(t.gates + " " + t.testsHelp)}
                </p>
                <div className="knowledge-scorecards">
                  {(["target", "paraphrase", "indirect", "locality"] as const).map(
                    (kind) => {
                      const rows = selected.results.filter(
                        (_, i) => selected.draft.evaluation[i].kind === kind,
                      );
                      return (
                        <div key={kind}>
                          <span>{t[kind]}</span>
                          <strong>
                            {rows.filter((r) => r.baseline?.passed).length}{" "}
                            <span>→</span>{" "}
                            {rows.filter((r) => r.candidate?.passed).length}
                            <small> / {rows.length}</small>
                          </strong>
                        </div>
                      );
                    },
                  )}
                </div>
                {selected.draft.evaluation.map((test, i) => (
                  <article className="knowledge-comparison" key={i}>
                    <header>
                      <span>{t[test.kind]}</span>
                      <h3>{test.prompt}</h3>
                    </header>
                    <p className="knowledge-rule-summary">{test.language !== "und" && test.language ? `${test.language.toUpperCase()} · ` : ""}{test.matchMode === "exact" ? t.exact : t.contains} · {test.expected}{test.aliases?.length ? ` / ${test.aliases.join(" / ")}` : ""}</p>
                    <div className="knowledge-comparison-grid">
                      <section>
                        <h4>{t.baseline}</h4>
                        {answer(selected.results[i]?.baseline || null)}
                      </section>
                      <section>
                        <h4>{t.candidate}</h4>
                        {answer(selected.results[i]?.candidate || null)}
                      </section>
                    </div>
                  </article>
                ))}
                {selected.provenance && (
                  <div className="knowledge-artifact">
                    <label>
                      {t.artifact}
                      <code>{selected.directory}/adapter</code>
                    </label>
                    <button
                      className="icon-button"
                      title={copied ? t.copied : t.copy}
                      aria-label={t.copy}
                      onClick={() => {
                        void navigator.clipboard
                          .writeText(selected.directory + "/adapter")
                          .then(() => {
                            setCopied(true);
                            setTimeout(() => setCopied(false), 2000);
                          })
                          .catch((e) => setError(String(e)));
                      }}
                    >
                      {copied ? <Check /> : <Copy />}
                    </button>
                    <details>
                      <summary>{t.fingerprint}</summary>
                      <code>{selected.provenance.sourceSha256}</code>
                      <p>{t.adapterHash}</p>
                      <code>{selected.provenance.adapterSha256}</code>
                    </details>
                  </div>
                )}
                {!!selected.logs.length && (
                  <details className="knowledge-logs">
                    <summary>{t.logs}</summary>
                    <pre>{selected.logs.join("\n")}</pre>
                  </details>
                )}
              </>
            )}
          </div>
          <div
            id="knowledge-history"
            role="tabpanel"
            aria-labelledby="knowledge-tab-history"
            hidden={view !== "history"}
            className="knowledge-page"
          >
            {!experiments.length ? (
              <div className="knowledge-empty">
                <History />
                <h2>{t.empty}</h2>
              </div>
            ) : (
              experiments.map((e) => (
                <button
                  key={e.id}
                  className="knowledge-history-item"
                  onClick={() => void command("select", { id: e.id })}
                  disabled={pending}
                >
                  <FlaskConical size={18} />
                  <span>
                    <strong>{e.draft.name}</strong>
                    <small>
                      {new Date(e.createdAt * 1000).toLocaleString(
                        language === "zh" ? "zh-CN" : language,
                      )}{" "}
                      · {e.draft.operation === "add" ? t.add : t.suppress}
                    </small>
                  </span>
                  <span className={`knowledge-status status-${e.status}`}>
                    {t.status[e.status]}
                  </span>
                </button>
              ))
            )}
          </div>
        </>
      )}
      <footer className="knowledge-footer">
        <span>{t.localOnly}</span>
        <span>
          {state?.busy
            ? t.status[
                experiments.find((e) => !terminal.has(e.status))?.status ||
                  "checking"
              ]
            : "MLX"}
        </span>
      </footer>
    </section>
  );
}
