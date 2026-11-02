import { Info, Stethoscope } from "lucide-react";
import { words, type Language } from "./i18n";

export type ModelPlanState = {
  runtimeBuffers?: {
    session: string;
    pid: number;
    kvBytes: number | null;
    computeBytes: number | null;
    outputBytes: number | null;
    workspaceBytes: number | null;
  } | null;
  backend: string;
  context: number | null;
  weightsBytes: number | null;
  residentEstimateBytes: number | null;
  kvBytes: number | null;
  workspaceBytes: number | null;
  kvBasis?: string | null;
  workspaceBasis?: string | null;
  retainedKvLimitBytes?: number | null;
  allocatorCacheLimitBytes?: number | null;
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
    n == null
      ? "—"
      : n >= 2 ** 30
        ? `${(n / 2 ** 30).toFixed(2)} GiB`
        : `${(n / 2 ** 20).toFixed(2)} MiB`;
  const rows = [
    [t.planBackend, value.backend],
    [t.contextLength, value.context?.toLocaleString() ?? "—"],
    [t.planWeights, bytes(value.weightsBytes)],
    [t.planResident, bytes(value.residentEstimateBytes)],
    [
      t.planKv,
      value.kvBytes == null ? "—" : `≈ ${bytes(value.kvBytes)}`,
      value.kvBasis === "strata_state_payload" ? t.planStateHelp : t.planKvHelp,
    ],
    [
      t.planWorkspace,
      value.workspaceBytes == null ? "—" : `≥ ${bytes(value.workspaceBytes)}`,
      t.planWorkspaceHelp,
    ],
    [
      t.planRetained,
      bytes(value.retainedKvLimitBytes ?? null),
      t.planLimitsHelp,
    ],
    [
      t.planAllocator,
      bytes(value.allocatorCacheLimitBytes ?? null),
      t.planLimitsHelp,
    ],
    [t.memoryReserve, bytes(value.osReserveBytes)],
    [t.planBudget, bytes(value.memoryBudgetBytes)],
    [t.planDisk, bytes(value.diskFreeBytes)],
  ];
  if (value.runtimeBuffers) {
    const r = value.runtimeBuffers;
    for (const [label, amount] of [
      [t.planReportedKv, r.kvBytes],
      [t.planReportedCompute, r.computeBytes],
      [t.planReportedOutput, r.outputBytes],
      [t.planReportedWorkspace, r.workspaceBytes],
    ] as const) {
      rows.push([
        label,
        amount == null ? "—" : `≈ ${bytes(amount)}`,
        t.planReportedHelp,
      ]);
    }
  }
  const warnings: Record<string, string> = {
    config_missing: t.planConfigMissing,
    model_missing: t.planModelMissing,
    backend_memory: t.planBackendMemory,
    reserve_exhausted: t.planReserve,
    weights_over_budget: t.planWeightsBudget,
    output_reservation: t.planOutput,
    disk_full: t.planDiskFull,
    state_workspace_budget: t.planStateBudget,
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
        {rows.map(([label, text, help]) => (
          <div key={label}>
            <dt>
              {label}{" "}
              {help && (
                <span title={help} aria-label={help}>
                  <Info size={11} />
                </span>
              )}
            </dt>
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
