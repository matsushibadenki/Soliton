import { useState } from "react";
import { Sparkles, Download, Info } from "lucide-react";
import type { Language } from "./i18n";
import { request } from "./client";
import type { EvaluationCase } from "./knowledge-types";
import { knowledgeWords } from "./knowledge-i18n";
export type GeneratedPlan = {
  id: string;
  revision: number;
  model: string;
  goal: string;
  sources: string;
  operation: string;
  summary: string;
  risks: string[];
  training: { prompt: string; response: string }[];
  evaluation: EvaluationCase[];
};
const labels = {
  en: {
    title: "OpenAI editing planner",
    key: "API key (session only)",
    model: "OpenAI model",
    goal: "Editing goal",
    sources: "Source material · include unrelated facts for locality tests",
    generate: "Generate plan & data",
    apply: "Review & import into draft",
    help: "Sends only your goal, sources, operation and language to OpenAI. API usage is billed. Keys are not saved; OPENAI_API_KEY on the Rust server can be used instead. Generated facts require source review. Suppression does not prove erasure.",
    review: "Generated proposal · review sources and every example",
    training: "Training examples",
    evaluation: "Held-out evaluation",
    stale: "Draft changed. Generate a new plan.",
    save: "Save your draft before planning.",
    error: "Planning failed",
  },
  ja: {
    title: "OpenAI 編集プランナー",
    key: "APIキー（保存しません）",
    model: "OpenAIモデル",
    goal: "編集の目的",
    sources: "根拠資料・周辺知識の確認に使う無関係の事実",
    generate: "計画・学習データを生成",
    apply: "確認して下書きへ取り込む",
    help: "目的・根拠資料・編集操作・言語をOpenAIに送信します。API利用料金が発生します。キーは保存せず、RustサーバーのOPENAI_API_KEYも使用できます。生成された事実は出典を確認してください。回答抑制は完全削除の証明にはなりません。",
    review: "生成案 · 出典と各学習例を確認してください",
    training: "学習例",
    evaluation: "独立した評価質問",
    stale: "下書きが変わりました。計画を再生成してください。",
    save: "計画生成前に下書きを保存してください。",
    error: "計画を生成できませんでした",
  },
  zh: {
    title: "OpenAI编辑规划器",
    key: "API密钥（不保存）",
    model: "OpenAI模型",
    goal: "编辑目标",
    sources: "来源材料 · 包含用于周边知识测试的无关事实",
    generate: "生成计划与训练数据",
    apply: "审核并导入草稿",
    help: "仅向OpenAI发送目标、来源材料、操作和语言。API使用会产生费用。密钥不保存，也可使用Rust服务器的OPENAI_API_KEY。请核实生成内容的来源。抑制回答不能证明彻底遗忘。",
    review: "生成建议 · 请审核来源及每个样本",
    training: "训练样本",
    evaluation: "独立评估问题",
    stale: "草稿已更改，请重新生成计划。",
    save: "请先保存草稿再生成计划。",
    error: "规划失败",
  },
};
export function KnowledgePlanner({
  language,
  operation,
  revision,
  plan,
  disabled,
  dirty,
  onRefresh,
  onImport,
}: {
  language: Language;
  operation: string;
  revision: number;
  plan: GeneratedPlan | null;
  disabled: boolean;
  dirty: boolean;
  onRefresh: () => Promise<void>;
  onImport: (id: string) => Promise<void>;
}) {
  const t = labels[language];
  const rules = knowledgeWords[language];
  const [key, setKey] = useState("");
  const [model, setModel] = useState("gpt-4.1");
  const [goal, setGoal] = useState("");
  const [sources, setSources] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function generate() {
    setBusy(true);
    setError("");
    const apiKey = key;
    setKey("");
    try {
      await request("/studio/knowledge", {
        action: "generatePlan",
        apiKey,
        model,
        goal,
        sources,
        operation,
        language,
        revision,
      });
      await onRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="knowledge-settings knowledge-planner">
      <summary>
        <Sparkles size={14} /> {t.title}{" "}
        <span title={t.help} aria-label={t.help} tabIndex={0}>
          <Info size={14} />
        </span>
      </summary>
      <p className="knowledge-hint">{t.help}</p>
      <fieldset disabled={disabled || busy}>
        <div className="knowledge-paths">
          <label>
            {t.key}
            <input
              type="password"
              autoComplete="off"
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
          </label>
          <label>
            {t.model}
            <input
              value={model}
              maxLength={120}
              onChange={(e) => setModel(e.target.value)}
            />
          </label>
        </div>
        <label>
          {t.goal}
          <textarea
            value={goal}
            maxLength={4096}
            onChange={(e) => setGoal(e.target.value)}
          />
        </label>
        <label>
          {t.sources}
          <textarea
            rows={5}
            value={sources}
            maxLength={16000}
            onChange={(e) => setSources(e.target.value)}
          />
        </label>
        {dirty && <p className="knowledge-hint">{t.save}</p>}
        <button
          type="button"
          disabled={dirty || !goal.trim() || !sources.trim() || !model.trim()}
          onClick={() => void generate()}
        >
          <Sparkles size={14} />
          {busy ? "…" : t.generate}
        </button>
      </fieldset>
      {error && (
        <p role="alert" className="knowledge-error">
          {t.error}: {error}
        </p>
      )}
      {plan && (
        <section>
          <h3>{t.review}</h3>
          <p style={{ whiteSpace: "pre-wrap" }}>{plan.summary}</p>
          <ul>
            {plan.risks.map((r, i) => (
              <li key={i}>{r}</li>
            ))}
          </ul>
          <details>
            <summary>
              {t.training} ({plan.training.length})
            </summary>
            {plan.training.map((r, i) => (
              <article key={i}>
                <p>{r.prompt}</p>
                <p>{r.response}</p>
              </article>
            ))}
          </details>
          <details>
            <summary>
              {t.evaluation} ({plan.evaluation.length})
            </summary>
            {plan.evaluation.map((r, i) => (
              <article key={i}>
                <p>
                  {rules[r.kind]} · {r.language || "und"}: {r.prompt}
                </p>
                <code>
                  {r.matchMode === "exact" ? rules.exact : rules.contains}: {r.expected}{r.aliases?.length ? ` / ${r.aliases.join(" / ")}` : ""} / − {r.forbidden || "—"}
                </code>
              </article>
            ))}
          </details>
          {(plan.revision !== revision || plan.operation !== operation) && (
            <p>{t.stale}</p>
          )}
          <button
            type="button"
            disabled={
              disabled ||
              busy ||
              dirty ||
              plan.revision !== revision ||
              plan.operation !== operation
            }
            onClick={() => void onImport(plan.id)}
          >
            <Download size={14} />
            {t.apply}
          </button>
        </section>
      )}
    </details>
  );
}
