import { useRef } from "react";
export type Layout = {
  engineWidth: number;
  apiWidth: number;
  logsHeight: number;
  workspaceOrder: ("chat" | "logs")[];
  workspaceActive: "chat" | "logs";
  workspaceSplit: "horizontal" | "vertical" | null;
  workspaceRatio: number;
};
export const defaultLayout: Layout = {
  engineWidth: 270,
  apiWidth: 300,
  logsHeight: 174,
  workspaceOrder: ["chat", "logs"],
  workspaceActive: "chat",
  workspaceSplit: null,
  workspaceRatio: 50,
};
export function ResizeHandle({
  axis,
  label,
  value,
  min,
  max,
  onCommit,
  variable,
  reverse = false,
}: {
  axis: "vertical" | "horizontal";
  label: string;
  value: number;
  min: number;
  max: number;
  onCommit: (value: number) => void | Promise<void>;
  variable: string;
  reverse?: boolean;
}) {
  const drag = useRef<{
    start: number;
    value: number;
    next: number;
    root: HTMLElement;
  } | null>(null);
  const previewVariable = variable.replace("--", "--preview-");
  const clamp = (n: number) => Math.round(Math.max(min, Math.min(max, n)));
  function finish(commit: boolean) {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    if (commit) {
      Promise.resolve(onCommit(d.next)).finally(() => {
        if (!drag.current) d.root.style.removeProperty(previewVariable);
      });
    } else d.root.style.removeProperty(previewVariable);
  }
  return (
    <div
      className={"resize-handle " + axis}
      role="separator"
      tabIndex={0}
      aria-label={label}
      aria-orientation={axis}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.currentTarget.focus();
        e.currentTarget.setPointerCapture(e.pointerId);
        const root = e.currentTarget.closest(".workbench") as HTMLElement;
        drag.current = {
          start: axis === "vertical" ? e.clientX : e.clientY,
          value,
          next: value,
          root,
        };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        d.next = clamp(
          d.value +
            ((axis === "vertical" ? e.clientX : e.clientY) - d.start) *
              (reverse ? -1 : 1),
        );
        d.root.style.setProperty(previewVariable, d.next + "px");
      }}
      onPointerUp={() => finish(true)}
      onPointerCancel={() => finish(false)}
      onLostPointerCapture={() => finish(false)}
      onKeyDown={(e) => {
        const keys =
          axis === "vertical"
            ? ["ArrowLeft", "ArrowRight"]
            : ["ArrowUp", "ArrowDown"];
        if (keys.includes(e.key)) {
          e.preventDefault();
          onCommit(
            clamp(value + (e.key === keys[0] ? -10 : 10) * (reverse ? -1 : 1)),
          );
        }
        if (e.key === "Home") {
          e.preventDefault();
          onCommit(min);
        }
        if (e.key === "End") {
          e.preventDefault();
          onCommit(max);
        }
      }}
    />
  );
}
