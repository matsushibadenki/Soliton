import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Columns2,
  Rows2,
  Combine,
  LayoutPanelLeft,
  ExternalLink,
  PanelLeftClose,
} from "lucide-react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { words, type Language } from "./i18n";
import type { Layout } from "./ResizeHandle";
import {
  WorkspaceTabs,
  type WorkspaceTab,
  hasWorkspaceDrag,
  readWorkspaceDrag,
} from "./WorkspaceTabs";
export type DockPlacement = "left" | "right" | "top" | "bottom" | "center";
export type WorkspaceGroup = {
  id: string;
  tabs: WorkspaceTab[];
  active: WorkspaceTab;
};
export const defaultGroups: WorkspaceGroup[] = [
  { id: "workspace", tabs: ["chat", "logs"], active: "chat" },
];
function DockMenu({
  language,
  disabled,
  split,
  onDock,
}: {
  language: Language;
  disabled: boolean;
  split: boolean;
  onDock: (placement: DockPlacement) => void;
}) {
  const t = words[language];
  const trigger = useRef<HTMLButtonElement>(null),
    menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{
    left: number;
    top: number;
  } | null>(null);
  useEffect(() => {
    if (!position) return;
    function close(e: PointerEvent) {
      if (
        !trigger.current?.contains(e.target as Node) &&
        !menu.current?.contains(e.target as Node)
      )
        setPosition(null);
    }
    function escape(e: KeyboardEvent) {
      if (e.key === "Escape") {
        setPosition(null);
        trigger.current?.focus();
      }
    }
    const resize = () => setPosition(null);
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", escape);
    window.addEventListener("resize", resize);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", escape);
      window.removeEventListener("resize", resize);
    };
  }, [position]);
  return (
    <div className="dock-menu">
      <button
        ref={trigger}
        className="icon-button"
        aria-label={t.dockLayout}
        title={t.dockLayout}
        aria-haspopup="menu"
        aria-expanded={!!position}
        onClick={() => {
          const rect = trigger.current!.getBoundingClientRect();
          setPosition(
            position
              ? null
              : {
                  left: Math.max(
                    16,
                    Math.min(innerWidth - 230, rect.right - 214),
                  ),
                  top: Math.max(16, rect.top - 220),
                },
          );
        }}
      >
        <LayoutPanelLeft size={15} />
      </button>
      {position &&
        createPortal(
          <div
            ref={menu}
            className="dock-menu-items"
            role="menu"
            style={{
              position: "fixed",
              left: position.left,
              top: position.top,
            }}
            onKeyDown={(e) => {
              const buttons = Array.from(
                menu.current!.querySelectorAll<HTMLButtonElement>(
                  "button:not(:disabled)",
                ),
              );
              const current = buttons.indexOf(
                document.activeElement as HTMLButtonElement,
              );
              if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
                e.preventDefault();
                buttons[
                  e.key === "Home"
                    ? 0
                    : e.key === "End"
                      ? buttons.length - 1
                      : (current +
                          (e.key === "ArrowUp" ? -1 : 1) +
                          buttons.length) %
                        buttons.length
                ]?.focus();
              }
            }}
          >
            {(["left", "right", "top", "bottom", "center"] as const).map(
              (place) => (
                <button
                  role="menuitem"
                  key={place}
                  disabled={disabled || (place === "center" && !split)}
                  onClick={() => {
                    setPosition(null);
                    onDock(place);
                  }}
                >
                  {place === "center" ? (
                    <Combine />
                  ) : place === "left" || place === "right" ? (
                    <Columns2 />
                  ) : (
                    <Rows2 />
                  )}
                  {t.dockPositions[place]}
                </button>
              ),
            )}
          </div>,
          document.body,
        )}
    </div>
  );
}

function SplitDivider({
  vertical,
  ratio,
  label,
  onCommit,
}: {
  vertical: boolean;
  ratio: number;
  label: string;
  onCommit: (ratio: number) => Promise<void>;
}) {
  const drag = useRef<{
    root: HTMLElement;
    rect: DOMRect;
    value: number;
  } | null>(null);
  function finish(commit: boolean) {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    if (commit)
      void onCommit(d.value).finally(() =>
        d.root.style.removeProperty("--dock-preview"),
      );
    else d.root.style.removeProperty("--dock-preview");
  }
  return (
    <div
      className="dock-divider"
      role="separator"
      tabIndex={0}
      aria-label={label}
      aria-orientation={vertical ? "horizontal" : "vertical"}
      aria-valuemin={25}
      aria-valuemax={75}
      aria-valuenow={ratio}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        const root = e.currentTarget.parentElement!;
        drag.current = {
          root,
          rect: root.getBoundingClientRect(),
          value: ratio,
        };
        e.currentTarget.setPointerCapture(e.pointerId);
        e.currentTarget.focus();
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const next =
          (vertical
            ? (e.clientY - d.rect.top) / d.rect.height
            : (e.clientX - d.rect.left) / d.rect.width) * 100;
        d.value = Math.round(Math.max(25, Math.min(75, next)));
        d.root.style.setProperty("--dock-preview", String(d.value));
      }}
      onPointerUp={() => finish(true)}
      onPointerCancel={() => finish(false)}
      onLostPointerCapture={() => finish(false)}
      onKeyDown={(e) => {
        const keys = vertical
          ? ["ArrowUp", "ArrowDown"]
          : ["ArrowLeft", "ArrowRight"];
        if (keys.includes(e.key) || ["Home", "End"].includes(e.key)) {
          e.preventDefault();
          void onCommit(
            e.key === "Home"
              ? 25
              : e.key === "End"
                ? 75
                : Math.max(
                    25,
                    Math.min(75, ratio + (e.key === keys[0] ? -5 : 5)),
                  ),
          );
        }
      }}
    />
  );
}
export function WorkspaceDock({
  groups,
  layout,
  detached,
  hosts,
  windowGroup,
  language,
  native,
  disabled,
  structureDisabled,
  onSelect,
  onMove,
  onFloat,
  onDockWindow,
  onDockTab,
  onResize,
  render,
  trace = false,
}: {
  groups: WorkspaceGroup[];
  layout: Layout;
  detached: string[];
  hosts: Record<string, string>;
  windowGroup: string | null;
  language: Language;
  native: boolean;
  disabled: boolean;
  structureDisabled: boolean;
  onSelect: (tab: WorkspaceTab) => void;
  onMove: (tab: WorkspaceTab, before: WorkspaceTab) => void;
  onFloat: (group: string) => void;
  onDockWindow: (group: string) => void;
  onDockTab: (
    tab: WorkspaceTab,
    target: WorkspaceTab,
    placement: DockPlacement,
  ) => void;
  onResize: (ratio: number) => Promise<void>;
  render: (tab: WorkspaceTab) => ReactNode;
  trace?: boolean;
}) {
  const t = words[language],
    ref = useRef<HTMLDivElement>(null);
  const [narrow, setNarrow] = useState(false);
  function clearDrag() {
    ref.current
      ?.querySelectorAll("[data-drag-over]")
      .forEach((slot) => slot.removeAttribute("data-drag-over"));
  }
  function showDrag(slot: HTMLElement) {
    // Native WebKit dragging must update hit targets in the current event turn.
    // This attribute is a transient preview; Rust still owns the dock layout.
    clearDrag();
    slot.setAttribute("data-drag-over", "true");
    if (native && trace) {
      const overlay = slot.querySelector<HTMLElement>(".dock-targets");
      const rect = overlay?.getBoundingClientRect();
      void invoke("trace_workspace_drag", {
        event: {
          phase: "slot-preview",
          element: overlay ? getComputedStyle(overlay).visibility : "missing",
          x: rect?.x,
          y: rect?.y,
          width: rect?.width,
          height: rect?.height,
        },
      }).catch(() => {});
    }
  }
  useEffect(() => {
    document.addEventListener("dragend", clearDrag, true);
    document.addEventListener("drop", clearDrag);
    return () => {
      document.removeEventListener("dragend", clearDrag, true);
      document.removeEventListener("drop", clearDrag);
    };
  }, []);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) =>
      setNarrow(entry.contentRect.width < 680),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const visible = windowGroup
    ? groups.filter((g) => hosts[g.id] === windowGroup)
    : groups;
  const split = visible.length > 1;
  const vertical = layout.workspaceSplit === "vertical" || narrow;
  function dock(
    group: WorkspaceGroup,
    place: DockPlacement,
    source = group.active,
  ) {
    const target =
      group.tabs.find((tab) => tab !== source) ||
      groups.flatMap((g) => g.tabs).find((tab) => tab !== source);
    if (target && !structureDisabled) onDockTab(source, target, place);
    clearDrag();
  }
  function traceSlot(phase: string, e: React.DragEvent) {
    if (native && trace)
      void invoke("trace_workspace_drag", {
        event: {
          phase,
          blocked: structureDisabled,
          hasDrag: hasWorkspaceDrag(e.dataTransfer),
          types: Array.from(e.dataTransfer.types),
          bubbles: e.bubbles,
          x: e.clientX,
          y: e.clientY,
          time: Date.now(),
        },
      }).catch(() => {});
  }
  return (
    <div
      ref={ref}
      className={
        "workspace-dock" +
        (split ? (vertical ? " split-rows" : " split-columns") : "")
      }
      style={{ "--dock-ratio": layout.workspaceRatio } as React.CSSProperties}
    >
      {visible.map((group, index) => (
        <div
          key={group.id}
          className="dock-slot"
          data-group={group.id}
          onDragEnterCapture={(e) => {
            traceSlot("slot-enter", e);
            if (!structureDisabled && hasWorkspaceDrag(e.dataTransfer))
              showDrag(e.currentTarget);
          }}
          onDragOverCapture={(e) => {
            traceSlot("slot-over", e);
            if (!structureDisabled && hasWorkspaceDrag(e.dataTransfer)) {
              e.preventDefault();
              showDrag(e.currentTarget);
            }
          }}
          onDragLeaveCapture={(e) => {
            // WebKit supplies null relatedTarget when an overlay replaces a child.
            // Keep the destination while the pointer is still inside this slot.
            const rect = e.currentTarget.getBoundingClientRect();
            const inside =
              e.clientX >= rect.left &&
              e.clientX < rect.right &&
              e.clientY >= rect.top &&
              e.clientY < rect.bottom;
            if (!inside && !e.currentTarget.contains(e.relatedTarget as Node))
              clearDrag();
          }}
          onDrop={() => clearDrag()}
          onDragEnd={() => clearDrag()}

          style={
            split
              ? {
                  gridArea: vertical
                    ? `${index * 2 + 1} / 1`
                    : `1 / ${index * 2 + 1}`,
                }
              : undefined
          }
        >
          {!windowGroup && detached.includes(group.id) ? (
            <div className="workspace-detached">
              <ExternalLink size={24} />
              <span>{group.tabs.map((id) => t[id]).join(" / ")}</span>
              <span>{t.workspaceFloating}</span>
              <div>
                <button disabled={disabled} onClick={() => onFloat(group.id)}>
                  <ExternalLink />
                  {t.showGroup}
                </button>
                <button
                  disabled={disabled}
                  onClick={() => onDockWindow(group.id)}
                >
                  <PanelLeftClose />
                  {t.dockGroup}
                </button>
              </div>
            </div>
          ) : (
            <section
              className="workspace dock-group"
              onFocusCapture={() => {
                if (
                  group.tabs.length === 1 &&
                  layout.workspaceActive !== group.active
                )
                  onSelect(group.active);
              }}
              aria-label={
                t.workspaceTabs +
                " · " +
                group.tabs.map((id) => t[id]).join(" / ")
              }
            >
              <div
                className="workspace-content"
                role="tabpanel"
                id={"workspace-content-" + group.id}
                aria-labelledby={"workspace-" + group.active}
              >
                {group.tabs.map((tab) => (
                  <div
                    key={tab}
                    hidden={group.active !== tab}
                    className="workspace-page"
                  >
                    {render(tab)}
                  </div>
                ))}
              </div>
              <WorkspaceTabs
                order={group.tabs}
                active={group.active}
                language={language}
                disabled={disabled}
                floating={!!windowGroup}
                native={native}
                contentId={"workspace-content-" + group.id}
                onSelect={onSelect}
                onMove={(tab, before) =>
                  group.tabs.includes(tab)
                    ? onMove(tab, before)
                    : !structureDisabled && onDockTab(tab, before, "center")
                }
                onFloat={() =>
                  windowGroup ? onDockWindow(group.id) : onFloat(group.id)
                }
                extra={
                  <DockMenu
                    language={language}
                    disabled={structureDisabled}
                    split={!!layout.workspaceSplit}
                    onDock={(place) => dock(group, place)}
                  />
                }
              />
            </section>
          )}
          {
            <div
              className="dock-targets"
              onDragOver={(e) => e.preventDefault()}
            >
              {(["left", "right", "top", "bottom", "center"] as const).map(
                (place) => (
                  <div
                    key={place}
                    className={"dock-target dock-" + place}
                    aria-label={t.dockPositions[place]}
                    onDragOver={(e) => {
                      e.preventDefault();
                      e.dataTransfer.dropEffect = "move";
                    }}
                    onDropCapture={(e) => {
                      traceSlot("target-drop", e);
                      e.preventDefault();
                      e.stopPropagation();
                      const tab = readWorkspaceDrag(e.dataTransfer);
                      if (tab === "chat" || tab === "logs")
                        dock(group, place, tab);
                      else clearDrag();
                    }}
                  >
                    {place === "center" ? (
                      <Combine />
                    ) : place === "left" || place === "right" ? (
                      <Columns2 />
                    ) : (
                      <Rows2 />
                    )}
                  </div>
                ),
              )}
            </div>
          }
        </div>
      ))}
      {split && (
        <SplitDivider
          label={t.resizeGroups}
          vertical={vertical}
          ratio={layout.workspaceRatio}
          onCommit={onResize}
        />
      )}
      {windowGroup && !visible.length && (
        <div className="workspace-detached">{t.dockGroup}</div>
      )}
    </div>
  );
}
