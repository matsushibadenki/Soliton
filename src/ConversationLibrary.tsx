import { useState } from "react";
import { Check, MessageSquare, Plus, Search, Trash2, X } from "lucide-react";
import { Modal } from "./PreferencesDialog";
import { words, type Language } from "./i18n";
export type ConversationIndex = {
  activeId: string;
  items: {
    id: string;
    title: string;
    updatedAt: number;
    messageCount: number;
  }[];
};
export function ConversationLibrary({
  library,
  language,
  disabled,
  onAction,
  onClose,
}: {
  library?: ConversationIndex;
  language: Language;
  disabled: boolean;
  onAction: (action: string, id?: string) => Promise<void>;
  onClose: () => void;
}) {
  const t = words[language];
  const [query, setQuery] = useState("");
  const [removing, setRemoving] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  async function perform(action: string, id?: string) {
    setSaving(true);
    setError("");
    try {
      await onAction(action, id);
      setRemoving("");
      if (action !== "deleteConversation") onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }
  const items =
    library?.items.filter((c) =>
      c.title.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    ) ?? [];
  return (
    <Modal
      title={t.conversationLibrary}
      language={language}
      className="conversation-library-dialog"
      onClose={onClose}
    >
      <div className="conversation-library-tools">
        <label>
          <Search size={15} />
          <input
            autoFocus
            type="search"
            aria-label={t.searchConversations}
            placeholder={t.searchConversations}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="icon-button"
          title={t.newChat}
          aria-label={t.newChat}
          disabled={disabled || saving}
          onClick={() => perform("newConversation")}
        >
          <Plus size={16} />
        </button>
      </div>
      {error && (
        <div className="notice error" role="alert">
          {error}
        </div>
      )}
      <div className="conversation-library-list">
        {items.length ? (
          items.map((c) => (
            <div
              key={c.id}
              className="conversation-library-row"
              data-active={c.id === library?.activeId}
            >
              <button
                type="button"
                className="conversation-library-select"
                disabled={disabled || saving}
                onClick={() => perform("switchConversation", c.id)}
              >
                <MessageSquare size={16} />
                <span>
                  <strong>{c.title || t.untitledConversation}</strong>
                  <small>
                    {c.updatedAt
                      ? new Date(c.updatedAt).toLocaleString(
                          language === "zh" ? "zh-CN" : language,
                        )
                      : "—"}{" "}
                    · {c.messageCount} {t.conversationMessages}
                  </small>
                </span>
                {c.id === library?.activeId && <Check size={15} />}
              </button>
              {removing === c.id ? (
                <div className="conversation-delete-confirm">
                  <span>{t.deleteConversationConfirm}</span>
                  <button
                    type="button"
                    disabled={disabled || saving}
                    onClick={() => perform("deleteConversation", c.id)}
                  >
                    {t.deleteConversation}
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t.cancelDeleteConversation}
                    onClick={() => setRemoving("")}
                  >
                    <X size={14} />
                  </button>
                </div>
              ) : (
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`${t.deleteConversation}: ${c.title || t.untitledConversation}`}
                  title={t.deleteConversation}
                  disabled={disabled || saving}
                  onClick={() => setRemoving(c.id)}
                >
                  <Trash2 size={14} />
                </button>
              )}
            </div>
          ))
        ) : (
          <div className="empty">{t.noConversations}</div>
        )}
      </div>
    </Modal>
  );
}
