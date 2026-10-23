import { Brain, Info, Wrench, Activity } from "lucide-react";
import { words, type Language } from "./i18n";
export type Metrics = {
  id: string;
  source: string;
  model: string;
  engine?: string;
  startedAt: number;
  status: string;
  elapsedMs: number | null;
  firstOutputMs: number | null;
  firstAnswerMs: number | null;
  finishReason: string | null;
  usage: {
    prompt: number | null;
    completion: number | null;
    total: number | null;
    cached: number | null;
    reasoning: number | null;
  } | null;
  timings: {
    promptMs: number | null;
    decodeMs: number | null;
    tokensPerSecond: number | null;
  } | null;
};
export type Inference = {
  reasoning: string;
  tools: { index: number; id: string; name: string; arguments: string }[];
  metrics: Metrics;
  truncated: boolean;
};
const seconds = (ms: number | null | undefined) =>
  ms == null ? "—" : `${(ms / 1000).toFixed(2)} s`;
const count = (n: number | null | undefined) =>
  n == null ? "—" : n.toLocaleString();
export function RequestMetrics({
  value: m,
  language,
}: {
  value: Metrics;
  language: Language;
}) {
  const t = words[language];
  const status =
    m.status === "running"
      ? t.inferenceRunning
      : m.status === "completed"
        ? t.inferenceCompleted
        : m.status === "cancelled"
          ? t.inferenceCancelled
          : t.inferenceError;
  const rows = [
    [t.requestId, m.id],
    [t.requestModel, m.model || "—"],
    [t.engineSelector, m.engine || "—"],
    [
      t.requestStarted,
      new Date(m.startedAt).toLocaleString(
        language === "zh" ? "zh-CN" : language,
      ),
    ],
    [t.requestElapsed, seconds(m.elapsedMs)],
    [t.firstOutput, seconds(m.firstOutputMs)],
    [t.firstAnswer, seconds(m.firstAnswerMs)],
    [t.promptTokens, count(m.usage?.prompt)],
    [t.outputTokens, count(m.usage?.completion)],
    [t.cachedTokens, count(m.usage?.cached)],
    [t.reasoningTokens, count(m.usage?.reasoning)],
    [t.totalTokens, count(m.usage?.total)],
    [t.promptTime, seconds(m.timings?.promptMs)],
    [t.decodeTime, seconds(m.timings?.decodeMs)],
    [
      t.engineSpeed,
      m.timings?.tokensPerSecond == null
        ? "—"
        : `${m.timings.tokensPerSecond.toFixed(1)} tok/s`,
    ],
    [t.finishReason, m.finishReason ?? "—"],
  ];
  return (
    <details className="request-metrics">
      <summary>
        <Activity size={12} />
        <span data-request-status={m.status}>{status}</span>
        <span>{m.status === "running" ? "…" : seconds(m.elapsedMs)}</span>
        {m.usage?.completion != null && <span>{m.usage.completion} tok</span>}
        {m.timings?.tokensPerSecond != null && (
          <span>{m.timings.tokensPerSecond.toFixed(1)} tok/s</span>
        )}
      </summary>
      <div
        className="request-metrics-help"
        title={t.requestMetricsHelp}
        tabIndex={0}
        aria-label={t.requestMetricsHelp}
      >
        <Info size={12} />
      </div>
      <dl>
        {rows.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}
export function InferenceView({
  value,
  language,
}: {
  value?: Inference;
  language: Language;
}) {
  const t = words[language];
  if (!value) return null;
  return (
    <>
      {value.reasoning && (
        <details className="inference-reasoning">
          <summary>
            <Brain size={14} />
            {t.reasoningContent}
          </summary>
          <pre>{value.reasoning}</pre>
        </details>
      )}
      {value.tools.map((tool) => (
        <details className="inference-tools" key={tool.index}>
          <summary>
            <Wrench size={14} />
            {t.toolCall} · {tool.name || "…"}
          </summary>
          <pre>{tool.arguments || "…"}</pre>
        </details>
      ))}
      {value.truncated && (
        <small className="inference-truncated">{t.inferenceTruncated}</small>
      )}
    </>
  );
}
export function RequestLog({
  requests = [],
  language,
}: {
  requests?: Metrics[];
  language: Language;
}) {
  const t = words[language];
  return (
    <section className="request-log">
      <h3>
        {t.requestHistory} <span>{requests.length}/100</span>
      </h3>
      <small
        title={t.requestHistoryHelp}
        tabIndex={0}
        aria-label={t.requestHistoryHelp}
      >
        <Info size={12} />
      </small>
      {requests.length ? (
        requests.map((m) => (
          <div key={m.id} className="request-log-row">
            <div>
              <span>{m.source === "chat" ? t.chat : "API"}</span>
              <time>
                {new Date(m.startedAt).toLocaleTimeString(
                  language === "zh" ? "zh-CN" : language,
                )}
              </time>
              <span>{m.model}</span>
            </div>
            <RequestMetrics value={m} language={language} />
          </div>
        ))
      ) : (
        <p className="request-empty">{t.noRequests}</p>
      )}
    </section>
  );
}
