import { useEffect, useState } from "react";
import { Info, Cpu } from "lucide-react";
import { words, type Language } from "./i18n";

import { MemoryHistory } from "./MemoryHistory";
import { MemoryUsage, type MemoryUsageState } from "./MemoryUsage";

export type MemoryValues = {
  context: number;
  promptChunk: number;
  keepFreeGb: number;
};
export type MemorySettings = MemoryValues & { automatic: boolean };
export type MemoryTuning = {
  automatic: MemoryValues;
  next: MemoryValues | null;
  active: MemoryValues | null;
};
export const defaultMemory: MemorySettings = {
  automatic: true,
  context: 8192,
  promptChunk: 64,
  keepFreeGb: 7,
};

export function MemoryControls({
  value,
  tuning,
  usage,
  memoryBytes,
  disabled,
  language,
  onSave,
  onRestart,
  running,
  generating,
}: {
  value: MemorySettings;
  tuning?: MemoryTuning;
  usage?: MemoryUsageState;
  memoryBytes: number | null;
  disabled: boolean;
  language: Language;
  onSave: (value: MemorySettings) => Promise<void>;
  onRestart: (value: MemorySettings) => Promise<void>;
  running: boolean;
  generating: boolean;
}) {
  const t = words[language];
  const [draft, setDraft] = useState(value);
  const [saving, setSaving] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    setDraft(value);
  }, [value.automatic, value.context, value.promptChunk, value.keepFreeGb]);
  const automatic = tuning?.automatic ?? defaultMemory;
  const displayed = draft.automatic ? automatic : draft;
  const changed =
    draft.automatic !== value.automatic ||
    draft.context !== value.context ||
    draft.promptChunk !== value.promptChunk ||
    draft.keepFreeGb !== value.keepFreeGb;
  const fields = [
    {
      id: "context",
      label: t.contextLength,
      min: 512,
      max: 262144,
      step: 1,
      unit: "tokens",
    },
    {
      id: "promptChunk",
      label: t.promptChunk,
      min: 64,
      max: 4096,
      step: 64,
      unit: "tokens",
    },
    {
      id: "keepFreeGb",
      label: t.memoryReserve,
      min: 4,
      max: memoryBytes ? Math.min(256, Math.ceil(memoryBytes / 1e9) - 1) : 256,
      step: 1,
      unit: "GB",
    },
  ] as const;
  const effective = tuning?.active ?? tuning?.next;
  return (
    <form
      className="memory-settings"
      aria-label={t.memorySettings}
      onSubmit={async (e) => {
        e.preventDefault();
        setSaving(true);
        setError("");
        try {
          const restart =
            (e.nativeEvent as SubmitEvent).submitter?.getAttribute("value") ===
            "restart";
          setRestarting(restart);
          if (restart) await onRestart(draft);
          else await onSave(draft);
        } catch (e) {
          setError((e as Error).message);
        } finally {
          setSaving(false);
          setRestarting(false);
        }
      }}
    >
      <div className="memory-heading">
        <strong>
          <Cpu size={14} />
          {t.memorySettings}
        </strong>
        <span>
          {memoryBytes ? `${Math.round(memoryBytes / 2 ** 30)} GiB` : "—"}
        </span>
        <span title={t.memoryHelp} tabIndex={0} aria-label={t.memoryHelp}>
          <Info size={14} />
        </span>
      </div>
      <MemoryUsage usage={usage} language={language} />
      <div className="memory-mode" role="group" aria-label={t.memoryMode}>
        {[true, false].map((automatic) => (
          <button
            type="button"
            key={String(automatic)}
            aria-pressed={draft.automatic === automatic}
            disabled={disabled || saving}
            onClick={() =>
              setDraft(
                automatic
                  ? { ...draft, automatic: true }
                  : { ...displayed, automatic: false },
              )
            }
          >
            {automatic ? t.memoryAuto : t.memoryManual}
          </button>
        ))}
      </div>
      <div className="memory-fields">
        {fields.map((field) => (
          <label key={field.id}>
            {field.label}
            <div className="memory-input">
              <input
                type="number"
                required
                aria-label={field.label}
                min={field.min}
                max={field.max}
                step={field.step}
                disabled={disabled || saving || draft.automatic}
                value={
                  Number.isFinite(displayed[field.id])
                    ? displayed[field.id]
                    : ""
                }
                onChange={(e) =>
                  setDraft({ ...draft, [field.id]: e.target.valueAsNumber })
                }
              />
              <small>{field.unit}</small>
            </div>
          </label>
        ))}
      </div>
      <div className="memory-footer">
        <span>
          {tuning?.active ? t.memoryActive : t.memoryNext}:{" "}
          <code>
            {effective
              ? `${effective.context} / ${effective.promptChunk} / ${effective.keepFreeGb} GB`
              : "—"}
          </code>
        </span>
        <div className="memory-actions">
          <button
            type="submit"
            disabled={disabled || saving || !changed || running}
          >
            {t.memoryApply}
          </button>
          <button
            className="primary"
            type="submit"
            value="restart"
            disabled={disabled || saving || generating || !changed}
          >
            {saving
              ? restarting
                ? t.memoryRestarting
                : t.memorySaving
              : t.memoryApplyRestart}
          </button>
        </div>
      </div>
      <MemoryHistory
        history={usage?.history}
        events={usage?.events}
        language={language}
      />
      {generating && <small className="memory-hint">{t.memoryWaitReply}</small>}
      {error && (
        <div role="alert" className="notice error">
          {error}
        </div>
      )}
    </form>
  );
}
