import { useEffect, useRef, type PointerEvent, type ReactNode } from "react";
import { Cpu, Link, Settings, X } from "lucide-react";
import { words, type Language } from "./i18n";
export type PreferenceTab = "general" | "engine" | "api";

export function Modal({
  title,
  language,
  children,
  onClose,
  className = "",
  dismissible = true,
  movable = false,
}: {
  title: string;
  language: Language;
  children: ReactNode;
  onClose: () => void;
  className?: string;
  dismissible?: boolean;
  movable?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const drag = useRef<{ pointer: number; x: number; y: number } | null>(null);

  function startMove(event: PointerEvent<HTMLElement>) {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
    const dialog = ref.current!;
    const rect = dialog.getBoundingClientRect();
    Object.assign(dialog.style, {
      width: `${rect.width}px`,
      height: `${rect.height}px`,
      margin: "0",
      inset: "auto",
      left: `${rect.left}px`,
      top: `${rect.top}px`,
    });
    drag.current = { pointer: event.pointerId, x: event.clientX - rect.left, y: event.clientY - rect.top };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function move(event: PointerEvent<HTMLElement>) {
    if (drag.current?.pointer !== event.pointerId) return;
    const dialog = ref.current!;
    const left = Math.max(0, Math.min(event.clientX - drag.current.x, innerWidth - dialog.offsetWidth));
    const top = Math.max(0, Math.min(event.clientY - drag.current.y, innerHeight - dialog.offsetHeight));
    dialog.style.left = `${left}px`;
    dialog.style.top = `${top}px`;
  }

  const headingId = "dialog-" + className;
  useEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    const fitViewport = () => {
      if (!movable || !dialog.style.left) return;
      const bounds = dialog.getBoundingClientRect();
      dialog.style.left = `${Math.max(0, Math.min(bounds.left, innerWidth - bounds.width))}px`;
      dialog.style.top = `${Math.max(0, Math.min(bounds.top, innerHeight - bounds.height))}px`;
    };
    if (movable) window.addEventListener("resize", fitViewport);
    return () => {
      window.removeEventListener("resize", fitViewport);
      dialog.close();
    };
  }, [movable]);
  return (
    <dialog
      ref={ref}
      className={"app-dialog " + className}
      aria-labelledby={headingId}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <header
        className="dialog-header"
        onPointerDown={movable ? startMove : undefined}
        onPointerMove={movable ? move : undefined}
        onLostPointerCapture={() => { drag.current = null; }}
      >
        <h2 id={headingId}>{title}</h2>
        {dismissible && <button
          className="icon-button"
          aria-label={words[language].close}
          onClick={onClose}
        >
          <X />
        </button>}
      </header>
      {children}
    </dialog>
  );
}
export function PreferencesDialog({
  language,
  tab,
  onTab,
  onClose,
  error,
  standalone = false,
  children,
}: {
  language: Language;
  tab: PreferenceTab;
  onTab: (tab: PreferenceTab) => void;
  onClose: () => void;
  error: string;
  standalone?: boolean;
  children: ReactNode;
}) {
  const t = words[language];
  const tabs = ["general", "engine", "api"] as const;
  const icons = { general: <Settings />, engine: <Cpu />, api: <Link /> };
  const content = (
    <>
      <div
        role="tablist"
        aria-label={t.preferences}
        className="preference-tabs"
      >
        {tabs.map((id, index) => (
          <button
            key={id}
            id={"preference-" + id}
            role="tab"
            aria-selected={tab === id}
            aria-controls="preference-content"
            tabIndex={tab === id ? 0 : -1}
            onClick={() => onTab(id)}
            onKeyDown={(e) => {
              if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
                e.preventDefault();
                const next =
                  e.key === "Home"
                    ? 0
                    : e.key === "End"
                      ? 2
                      : (index + (e.key === "ArrowRight" ? 1 : 2)) % 3;
                onTab(tabs[next]);
                document.getElementById("preference-" + tabs[next])?.focus();
              }
            }}
          >
            {icons[id]}
            {t[id]}
          </button>
        ))}
      </div>
      {error && (
        <div className="notice error" role="alert">
          {error}
        </div>
      )}
      <div
        className="preference-content"
        id="preference-content"
        role="tabpanel"
        aria-labelledby={"preference-" + tab}
      >
        {children}
      </div>
    </>
  );
  if (standalone) {
    return <main className="preferences-dialog preferences-window" aria-label={t.preferences}>
      {content}
    </main>;
  }
  return (
    <Modal
      title={t.preferences.replace("…", "")}
      language={language}
      onClose={onClose}
      className="preferences-dialog"
      movable
    >
      {content}
    </Modal>
  );
}
