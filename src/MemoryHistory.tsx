import { useState } from "react";
import { Info } from "lucide-react";
import { words, type Language } from "./i18n";

export type MemoryPoint = {
  at: number;
  macUsedBytes: number | null;
  engineUsedBytes: number | null;
  pid: number | null;
  inferring: boolean;
};
export type MemoryEvent = {
  at: number;
  kind:
    | "settings"
    | "engineStart"
    | "engineStop"
    | "inferenceStart"
    | "inferenceEnd";
  context: number | null;
};
const bytes = (n: number | null | undefined) =>
  n == null
    ? "—"
    : n < 2 ** 30
      ? `${(n / 2 ** 20).toFixed(1)} MiB`
      : `${(n / 2 ** 30).toFixed(2)} GiB`;
export function MemoryHistory({
  history = [],
  events = [],
  language,
}: {
  history?: MemoryPoint[];
  events?: MemoryEvent[];
  language: Language;
}) {
  const t = words[language];
  const [minutes, setMinutes] = useState(2);
  const [hover, setHover] = useState<number | null>(null);
  const end = history.at(-1)?.at ?? Date.now();
  const start = end - minutes * 60_000;
  const points = history.filter((p) => p.at >= start);
  const visibleEvents = events.filter((e) => e.at >= start && e.at <= end);
  const x = (at: number) => 48 + ((at - start) / (end - start)) * 536;
  const clock = (at: number) =>
    new Date(at).toLocaleTimeString(language === "zh" ? "zh-CN" : language);
  const selected = points.length
    ? points.reduce((a, b) =>
        Math.abs(b.at - (hover ?? end)) < Math.abs(a.at - (hover ?? end))
          ? b
          : a,
      )
    : undefined;
  const eventName = (e: MemoryEvent) =>
    `${t[e.kind]}${e.context ? ` · ${e.context}` : ""}`;
  // Include intervals that began before the visible time window; short requests still appear.
  const bands: [number, number][] = [];
  const firstInference = events.find(
    (e) => e.kind === "inferenceStart" || e.kind === "inferenceEnd",
  );
  let active =
      points[0]?.inferring &&
      (!firstInference || firstInference.kind === "inferenceEnd")
        ? 1
        : 0,
    since = start;
  for (const e of events) {
    if (e.kind === "inferenceStart") {
      if (active++ === 0) since = e.at;
    }
    if (
      e.kind === "inferenceEnd" &&
      active > 0 &&
      --active === 0 &&
      e.at >= start
    )
      bands.push([Math.max(start, since), Math.min(end, e.at)]);
  }
  if (active > 0) bands.push([Math.max(start, since), end]);
  return (
    <section className="memory-history" aria-label={t.memoryHistory}>
      <div className="memory-history-heading">
        <strong>{t.memoryHistory}</strong>
        <span
          title={t.memoryHistoryHelp}
          tabIndex={0}
          aria-label={t.memoryHistoryHelp}
        >
          <Info size={12} />
        </span>
        <div role="group" aria-label={t.memoryTimeRange}>
          {[2, 5, 10].map((m) => (
            <button
              type="button"
              key={m}
              aria-pressed={minutes === m}
              onClick={() => {
                setMinutes(m);
                setHover(null);
              }}
            >
              {m}
              {t.memoryMinutes}
            </button>
          ))}
        </div>
      </div>
      {!points.some(
        (p) => p.macUsedBytes != null || p.engineUsedBytes != null,
      ) ? (
        <small className="memory-history-empty">{t.memoryHistoryWaiting}</small>
      ) : (
        <>
          <div className="memory-history-readout">
            <time>{selected ? clock(selected.at) : "—"}</time>
            <span className="mac-series">
              {t.memoryMac}: {bytes(selected?.macUsedBytes)}
            </span>
            <span className="engine-series">
              {t.memoryEngine}: {bytes(selected?.engineUsedBytes)}
            </span>
          </div>
          <svg
            className="memory-history-chart"
            viewBox="0 0 600 226"
            role="img"
            aria-label={t.memoryHistory}
            onPointerMove={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              setHover(
                start +
                  Math.max(
                    0,
                    Math.min(
                      1,
                      (((e.clientX - r.left) / r.width) * 600 - 48) / 536,
                    ),
                  ) *
                    (end - start),
              );
            }}
            onPointerLeave={() => setHover(null)}
          >
            {(
              [
                ["macUsedBytes", t.memoryMac, 10],
                ["engineUsedBytes", t.memoryEngine, 116],
              ] as const
            ).map(([key, label, top]) => {
              const values = points.flatMap((p) =>
                p[key] == null ? [] : [p[key]!],
              );
              const low = values.length ? Math.min(...values) : 0;
              const high = values.length ? Math.max(...values) : 0;
              const margin = Math.max(
                (high - low) * 0.15,
                key === "macUsedBytes" ? 64 * 2 ** 20 : 2 ** 20,
              );
              const min = Math.max(0, low - margin),
                max = high + margin;
              const y = (n: number) =>
                top + 78 - ((n - min) / (max - min)) * 54;
              let path = "",
                previous: MemoryPoint | undefined;
              for (const p of points) {
                const value = p[key];
                if (value == null) {
                  previous = undefined;
                  continue;
                }
                const connected =
                  previous &&
                  p.at - previous.at <= 3500 &&
                  (key !== "engineUsedBytes" || p.pid === previous.pid);
                path += `${connected ? "L" : "M"}${x(p.at).toFixed(2)},${y(value).toFixed(2)} `;
                previous = p;
              }
              return (
                <g
                  key={key}
                  className={
                    key === "macUsedBytes" ? "mac-series" : "engine-series"
                  }
                >
                  <text x="48" y={top + 12} className="history-series-title">
                    {label}
                  </text>
                  <text x="584" y={top + 12} textAnchor="end">
                    {values.length ? `${bytes(min)} – ${bytes(max)}` : "—"}
                  </text>
                  <path
                    className="history-grid"
                    d={`M48 ${top + 24}H584 M48 ${top + 78}H584`}
                  />
                  {bands.map(
                    ([a, b], i) =>
                      b >= a && (
                        <rect
                          key={i}
                          className="history-inference"
                          x={x(a)}
                          y={top + 18}
                          width={Math.max(1, x(b) - x(a))}
                          height="64"
                        />
                      ),
                  )}
                  {visibleEvents
                    .filter(
                      (e) =>
                        e.kind !== "inferenceStart" &&
                        e.kind !== "inferenceEnd",
                    )
                    .map((e, i) => (
                      <line
                        key={i}
                        className={`history-event ${e.kind}`}
                        x1={x(e.at)}
                        x2={x(e.at)}
                        y1={top + 18}
                        y2={top + 82}
                      >
                        <title>
                          {clock(e.at)} · {eventName(e)}
                        </title>
                      </line>
                    ))}
                  <path
                    data-history-series={key}
                    className="history-line"
                    d={path}
                  />
                  {selected && (
                    <line
                      className="history-cursor"
                      x1={x(selected.at)}
                      x2={x(selected.at)}
                      y1={top + 18}
                      y2={top + 82}
                    />
                  )}
                  {selected?.[key] != null && (
                    <circle
                      cx={x(selected.at)}
                      cy={y(selected[key]!)}
                      r="2.5"
                    />
                  )}
                </g>
              );
            })}
            <text x="48" y="223">
              {clock(start)}
            </text>
            <text x="584" y="223" textAnchor="end">
              {clock(end)}
            </text>
          </svg>
          <div className="memory-history-legend">
            <span>
              <i className="history-inference-key" />
              {t.memoryInferring}
            </span>
            <span>
              <i className="history-settings-key" />
              {t.settings}
            </span>
            <span>
              <i className="history-engine-key" />
              {t.memoryLifecycle}
            </span>
          </div>
          <div className="memory-history-events">
            {visibleEvents
              .slice(-4)
              .reverse()
              .map((e, i) => (
                <span
                  key={`${e.at}-${i}`}
                  title={`${clock(e.at)} · ${eventName(e)}`}
                >
                  <time>{clock(e.at)}</time> {eventName(e)}
                </span>
              ))}
          </div>
        </>
      )}
    </section>
  );
}
