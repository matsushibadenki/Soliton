import { Info } from "lucide-react";
import { words, type Language } from "./i18n";
export type VllmInspection = {
  status: "verified" | "unverified" | "blocked";
  reason: string;
  architecture: string | null;
  fitsMemory: boolean | null;
  estimatedResidentBytes: number | null;
  context: number;
  checkedAt: number;
};
export function VllmInspectionView({
  value,
  language,
}: {
  value: VllmInspection;
  language: Language;
}) {
  const t = words[language];
  const reasons: Record<string, string> = {
    evidence_identity_mismatch: t.vllmIdentityMismatch,
    evidence_expired: t.vllmExpired,
    evidence_limit_exceeded: t.vllmLimit,
    evidence_corrupt: t.vllmCorrupt,
    evidence_permissions: t.vllmPermissions,
    backend_fingerprint_failed: t.vllmFingerprint,
    model_memory_budget: t.vllmBudget,
    inspection_failed: t.vllmInspectFailed,
  };
  const status =
    value.status === "verified"
      ? t.vllmVerified
      : value.status === "unverified"
        ? t.vllmUnverified
        : t.vllmBlocked;
  return (
    <section
      className={
        "notice vllm-inspection" + (value.status === "blocked" ? " error" : "")
      }
      aria-label={t.vllmInspect}
    >
      <div className="value-row" role="status">
        <strong data-inspection-status={value.status}>{status}</strong>
        <span
          title={reasons[value.reason] || t.vllmInspectHelp}
          aria-label={reasons[value.reason] || t.vllmInspectHelp}
          tabIndex={0}
        >
          <Info size={13} />
        </span>
        <small>
          {new Date(value.checkedAt).toLocaleTimeString(
            language === "zh" ? "zh-CN" : language,
          )}
        </small>
      </div>
      {value.architecture && (
        <div className="value-row">
          <span>{t.vllmArchitecture}</span>
          <code>{value.architecture}</code>
        </div>
      )}
      {value.estimatedResidentBytes != null && (
        <div className="value-row">
          <span>{t.vllmEstimated}</span>
          <code>{(value.estimatedResidentBytes / 2 ** 30).toFixed(2)} GiB</code>
        </div>
      )}
    </section>
  );
}
