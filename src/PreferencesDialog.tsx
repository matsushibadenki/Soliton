import { useEffect, useRef, type ReactNode } from "react";
import { Cpu, Link, Settings, X } from "lucide-react";
import { words, type Language } from "./i18n";
export type PreferenceTab = "general" | "engine" | "api";

export function Modal({
  title,
  language,
  children,
  onClose,
  className = "",
}: {
  title: string;
  language: Language;
  children: ReactNode;
  onClose: () => void;
  className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const headingId = "dialog-" + className;
  useEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    return () => dialog.close();
  }, []);
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
      <header className="dialog-header">
        <h2 id={headingId}>{title}</h2>
        <button
          className="icon-button"
          aria-label={words[language].close}
          onClick={onClose}
        >
          <X />
        </button>
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
  children,
}: {
  language: Language;
  tab: PreferenceTab;
  onTab: (tab: PreferenceTab) => void;
  onClose: () => void;
  error: string;
  children: ReactNode;
}) {
  const t = words[language];
  const tabs = ["general", "engine", "api"] as const;
  const icons = { general: <Settings />, engine: <Cpu />, api: <Link /> };
  return (
    <Modal
      title={t.preferences.replace("…", "")}
      language={language}
      onClose={onClose}
      className="preferences-dialog"
    >
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
    </Modal>
  );
}
