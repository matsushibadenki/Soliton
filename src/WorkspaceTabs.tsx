import { useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  ExternalLink,
  FileText,
  MessageSquare,
  PanelLeftClose,
} from "lucide-react";
import { words, type Language } from "./i18n";
export type WorkspaceTab = "chat" | "logs";
export const workspaceDragType = "application/x-strata-workspace-tab";
const dragPrefix = "strata-workspace-tab:";
export function hasWorkspaceDrag(data: DataTransfer) {
  return (
    data.types.includes(workspaceDragType) || data.types.includes("text/plain")
  );
}
export function readWorkspaceDrag(data: DataTransfer): WorkspaceTab | null {
  const custom = data.getData(workspaceDragType);
  const plain = data.getData("text/plain");
  const value =
    custom ||
    (plain.startsWith(dragPrefix) ? plain.slice(dragPrefix.length) : "");
  return value === "chat" || value === "logs" ? value : null;
}
export function WorkspaceTabs({
  order,
  active,
  language,
  disabled,
  floating,
  native,
  onSelect,
  onMove,
  onFloat,
  extra,
  contentId = "workspace-content",
}: {
  order: WorkspaceTab[];
  active: WorkspaceTab;
  language: Language;
  disabled: boolean;
  floating: boolean;
  native: boolean;
  onSelect: (tab: WorkspaceTab) => void;
  onMove: (tab: WorkspaceTab, before: WorkspaceTab) => void;
  onFloat: () => void;
  extra?: React.ReactNode;
  contentId?: string;
}) {
  const t = words[language];
  const [target, setTarget] = useState<WorkspaceTab | null>(null);
  function move(tab: WorkspaceTab, destination: WorkspaceTab) {
    if (tab !== destination && !disabled) onMove(tab, destination);
    setTarget(null);
  }
  function swap() {
    if (order.length > 1) onMove(order[1], order[0]);
  }
  return (
    <div className="workspace-bar">
      <div
        className="workspace-tabs"
        role="tablist"
        aria-label={t.workspaceTabs}
      >
        {order.map((id, index) => (
          <button
            key={id}
            role="tab"
            id={"workspace-" + id}
            className={target === id ? "drop-target" : ""}
            aria-selected={active === id}
            aria-controls={contentId}
            tabIndex={active === id ? 0 : -1}
            draggable={!disabled}
            disabled={disabled}
            title={t.reorderTabs}
            onClick={() => onSelect(id)}
            onDragStart={(e) => {
              e.dataTransfer.setData(workspaceDragType, id);
              e.dataTransfer.setData("text/plain", dragPrefix + id);
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragOverCapture={(e) => {
              if (hasWorkspaceDrag(e.dataTransfer)) {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                setTarget(id);
              }
            }}
            onDragLeave={() => setTarget(null)}
            onDropCapture={(e) => {
              e.preventDefault();
              const source = readWorkspaceDrag(e.dataTransfer);
              if (source === "chat" || source === "logs")
                move(
                  order.length > 1 && order.includes(source)
                    ? order[1]
                    : source,
                  order.length > 1 && order.includes(source) ? order[0] : id,
                );
            }}
            onDragEnd={() => {
              setTarget(null);
            }}
            onKeyDown={(e) => {
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key))
                return;
              e.preventDefault();
              if (e.altKey && ["ArrowLeft", "ArrowRight"].includes(e.key)) {
                if (
                  (e.key === "ArrowLeft" && index > 0) ||
                  (e.key === "ArrowRight" && index < order.length - 1)
                )
                  swap();
                return;
              }
              const next =
                e.key === "Home"
                  ? order[0]
                  : e.key === "End"
                    ? order.at(-1)!
                    : order[(index + 1) % order.length];
              onSelect(next);
              document.getElementById("workspace-" + next)?.focus();
            }}
          >
            {id === "chat" ? <MessageSquare /> : <FileText />}
            {t[id]}
          </button>
        ))}
      </div>
      <div className="workspace-tools">
        {extra}
        <button
          className="icon-button"
          aria-label={t.moveTabLeft}
          title={t.moveTabLeft}
          disabled={disabled || order[0] === active}
          onClick={swap}
        >
          <ArrowLeft />
        </button>
        <button
          className="icon-button"
          aria-label={t.moveTabRight}
          title={t.moveTabRight}
          disabled={disabled || order.at(-1) === active}
          onClick={swap}
        >
          <ArrowRight />
        </button>
        <button
          className="icon-button"
          aria-label={floating ? t.dockGroup : t.floatGroup}
          title={floating ? t.dockGroup : t.floatGroup}
          disabled={!native || disabled}
          onClick={onFloat}
        >
          {floating ? <PanelLeftClose /> : <ExternalLink />}
        </button>
      </div>
    </div>
  );
}
