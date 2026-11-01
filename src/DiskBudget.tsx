import { Info } from "lucide-react";
import { words, type Language } from "./i18n";
export type DiskPlan = {
  target: string;
  requiredBytes: number;
  freeBytes: number | null;
  shortfallBytes: number | null;
  status: "unknown" | "insufficient" | "sufficient";
};
export function DiskBudget({
  value,
  language,
}: {
  value: DiskPlan;
  language: Language;
}) {
  const t = words[language];
  const gb = (n: number | null) =>
    n == null ? "—" : n > 0 && n < 1e8 ? "<0.1 GB" : `${(n / 1e9).toFixed(1)} GB`;
  return (
    <div className="disk-budget">
      <div className="value-row">
        <span>
          {t.diskTarget}{" "}
          <span title={value.target} aria-label={value.target} tabIndex={0}>
            <Info size={13} />
          </span>
        </span>
        <code title={value.target}>
          {value.target.split(/[\\/]/).filter(Boolean).pop() || value.target}
        </code>
      </div>
      <div className="value-row">
        <span>{t.freeDisk}</span>
        <code>{gb(value.freeBytes)}</code>
      </div>
      <div className="value-row">
        <span>
          {t.diskRequired}{" "}
          <span title={t.diskBudgetHelp}>
            <Info size={13} />
          </span>
        </span>
        <code>{gb(value.requiredBytes)}</code>
      </div>
      {value.status === "insufficient" && (
        <p className="setup-warning" role="status">
          {t.diskShort} · {t.diskDeficit}: {gb(value.shortfallBytes)}
        </p>
      )}
      {value.status === "unknown" && (
        <p className="setup-warning" role="status">
          {t.diskUnknown}
        </p>
      )}
    </div>
  );
}
