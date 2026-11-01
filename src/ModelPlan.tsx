import { Info, Stethoscope } from "lucide-react";
import { words, type Language } from "./i18n";

export type ModelPlanState = {
  backend: string;
  context: number | null;
  weightsBytes: number | null;
  residentEstimateBytes: number | null;
  kvBytes: number | null;
  workspaceBytes: number | null;
  osReserveBytes: number;
  memoryBudgetBytes: number | null;
  diskFreeBytes: number | null;
  warnings: string[];
  assessment: "incomplete" | "attention";
};
export function ModelPlan({
  value,
  language,
}: {
  value: ModelPlanState;
  language: Language;
}) {
  const t = words[language];
  const bytes = (n: number | null) =>
    n === null ? "—" : `${(n / 2 ** 30).toFixed(2)} GiB`;
  const rows = [
    [t.planBackend, value.backend],
    [t.contextLength, value.context?.toLocaleString() ?? "—"],
    [t.planWeights, bytes(value.weightsBytes)],
    [t.planResident, bytes(value.residentEstimateBytes)],
    [t.planKv, bytes(value.kvBytes)],
    [t.planWorkspace, bytes(value.workspaceBytes)],
    [t.memoryReserve, bytes(value.osReserveBytes)],
    [t.planBudget, bytes(value.memoryBudgetBytes)],
    [t.planDisk, bytes(value.diskFreeBytes)],
  ];
  const warnings: Record<string, string> = {
    config_missing: t.planConfigMissing,
    model_missing: t.planModelMissing,
    backend_memory: t.planBackendMemory,
    reserve_exhausted: t.planReserve,
    weights_over_budget: t.planWeightsBudget,
    output_reservation: t.planOutput,
    disk_full: t.planDiskFull,
  };
  return (
    <details className="model-plan">
      <summary>
        <Stethoscope size={15} /> {t.planTitle}{" "}
        <span className="muted">
          {value.assessment === "attention"
            ? t.planAttention
            : t.planIncomplete}
        </span>
        <span title={t.planHelp} aria-label={t.planHelp}>
          <Info size={13} />
        </span>
      </summary>
      <dl>
        {rows.map(([label, text]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{text}</dd>
          </div>
        ))}
      </dl>
      {value.warnings.length > 0 && (
        <ul>
          {value.warnings.map((code) => (
            <li key={code}>{warnings[code] ?? t.planAttention}</li>
          ))}
        </ul>
      )}
    </details>
  );
}
