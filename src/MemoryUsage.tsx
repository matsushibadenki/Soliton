import type { MemoryPoint, MemoryEvent } from "./MemoryHistory";
import { Info } from "lucide-react";
import { words, type Language } from "./i18n";

type Sample = {
  sampledAt: number;
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
  freeBytes: number;
};
export type MemoryUsageState = {
  history?: MemoryPoint[];
  events?: MemoryEvent[];
  current: Sample | null;
  beforeChange: Sample | null;
  engine?: {
    current: { sampledAt: number; pid: number; usedBytes: number } | null;
    beforeChange: { sampledAt: number; pid: number; usedBytes: number } | null;
  };
};
const gib = (n: number) => `${(n / 2 ** 30).toFixed(1)} GiB`;
const engineBytes = (n: number) =>
  n < 2 ** 30
    ? `${(n / 2 ** 20).toFixed(1)} MiB`
    : `${(n / 2 ** 30).toFixed(2)} GiB`;
function engineDelta(n: number) {
  const amount = Number((Math.abs(n) / 2 ** 20).toFixed(1));
  return `${amount === 0 ? "" : n > 0 ? "+" : "−"}${amount.toFixed(1)} MiB`;
}
function delta(n: number) {
  const amount = Number((Math.abs(n) / 2 ** 30).toFixed(2));
  return `${amount === 0 ? "" : n > 0 ? "+" : "−"}${amount.toFixed(2)} GiB`;
}

export function MemoryUsage({
  usage,
  language,
}: {
  usage?: MemoryUsageState;
  language: Language;
}) {
  const t = words[language];
  const now = usage?.current;
  const before = usage?.beforeChange;
  return (
    <section className="memory-usage" aria-label={t.memoryLive}>
      <div className="memory-live-heading">
        <span>{t.memoryLive}</span>
        <span
          title={t.memoryUsageHelp}
          tabIndex={0}
          aria-label={t.memoryUsageHelp}
        >
          <Info size={12} />
        </span>
        <small>
          {now
            ? new Date(now.sampledAt).toLocaleTimeString(
                language === "zh" ? "zh-CN" : language,
              )
            : t.memoryUnavailable}
        </small>
      </div>
      <div className="memory-live-values">
        {(
          [
            ["usedBytes", t.memoryUsed],
            ["availableBytes", t.memoryAvailable],
          ] as const
        ).map(([key, label]) => (
          <div key={key} className="memory-live-stat">
            <span>{label}</span>
            <strong data-memory={key}>{now ? gib(now[key]) : "—"}</strong>
            <small
              title={
                before
                  ? new Date(before.sampledAt).toLocaleTimeString()
                  : undefined
              }
            >
              {t.memoryBefore}: {before ? gib(before[key]) : "—"}
              <span data-delta={key}>
                {" "}
                {t.memoryDelta}:{" "}
                {now && before ? delta(now[key] - before[key]) : "—"}
              </span>
            </small>
          </div>
        ))}
      </div>
      <div className="memory-engine memory-live-stat">
        <span>
          {t.memoryEngine}{" "}
          <span
            title={t.memoryEngineHelp}
            tabIndex={0}
            aria-label={t.memoryEngineHelp}
          >
            <Info size={12} />
          </span>
        </span>
        <strong data-memory="engine">
          {usage?.engine?.current
            ? engineBytes(usage.engine.current.usedBytes)
            : "—"}
        </strong>
        <small
          title={
            usage?.engine?.beforeChange
              ? new Date(
                  usage.engine.beforeChange.sampledAt,
                ).toLocaleTimeString()
              : undefined
          }
        >
          {t.memoryBefore}:{" "}
          {usage?.engine?.beforeChange
            ? engineBytes(usage.engine.beforeChange.usedBytes)
            : "—"}
          <span data-delta="engine">
            {t.memoryDelta}:{" "}
            {usage?.engine?.current && usage.engine.beforeChange
              ? engineDelta(
                  usage.engine.current.usedBytes -
                    usage.engine.beforeChange.usedBytes,
                )
              : "—"}
          </span>
        </small>
      </div>
      <div
        className="memory-meter"
        role="meter"
        aria-label={t.memoryUsed}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={
          now ? Math.round((now.usedBytes / now.totalBytes) * 100) : undefined
        }
        aria-valuetext={
          now
            ? `${gib(now.usedBytes)} / ${gib(now.totalBytes)}`
            : t.memoryUnavailable
        }
      >
        <i
          style={{
            width: now ? `${(now.usedBytes / now.totalBytes) * 100}%` : "0%",
          }}
        />
      </div>
    </section>
  );
}
