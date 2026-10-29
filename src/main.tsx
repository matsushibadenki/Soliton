import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
  Sun,
  Moon,
  History,
  Cpu,
  MessageSquare,
  Link,
  FileText,
  ExternalLink,
  PanelLeftClose,
  Play,
  Square,
  Send,
  Copy,
  Check,
  Info,
  Trash2,
  Plus,
  Monitor,
  Layers,
  FolderOpen,
  FolderSearch,
  Download,
  Settings as SettingsIcon,
} from "lucide-react";
import {
  ConversationLibrary,
  type ConversationIndex,
} from "./ConversationLibrary";
import {
  InferenceView,
  RequestMetrics,
  RequestLog,
  type Inference,
  type Metrics,
} from "./InferenceView";
import { words, type Language } from "./i18n";
import { action, connect, desktop, request, streamChat, token } from "./client";
import "./style.css";
import { VllmInspectionView, type VllmInspection } from "./VllmInspection";
import { SetupDialog, type Inspection, type Preparation } from "./SetupDialog";
import { defaultLayout, type Layout } from "./ResizeHandle";
import type { WorkspaceTab } from "./WorkspaceTabs";
import {
  WorkspaceDock,
  defaultGroups,
  type WorkspaceGroup,
  type DockPlacement,
} from "./WorkspaceDock";
import { AppMenus, type AppMenu } from "./AppMenus";
import {
  Modal,
  PreferencesDialog,
  type PreferenceTab,
} from "./PreferencesDialog";
import { ModelLibraryDialog } from "./ModelLibraryDialog";
import {
  MemoryControls,
  defaultMemory,
  type MemorySettings,
  type MemoryTuning,
} from "./MemoryControls";
import { MemoryUsage, type MemoryUsageState } from "./MemoryUsage";
import { MemoryHistory } from "./MemoryHistory";
type Settings = {
  engine: "strata" | "vllmApple";
  vllm: {
    root: string;
    python: string;
    model: string;
    backend: "mlx_lm" | "vllm_metal";
    executable: string;
    evidence: string;
    context: number;
    kvCacheMiB: number;
    allocatorCacheMiB: number;
  };
  strataRoot: string;
  python: string;
  config: string;
  mock: boolean;
  temperature: number;
  maxTokens: number;
  language: Language;
  chatFontSize: number;
  theme: "light" | "dark";
  modelDirectories: string[];
  metalMemory: MemorySettings;
};
type Message = { role: string; content: string; inference?: Inference };
type Snapshot = {
  vllmInspection?: VllmInspection | null;
  requests?: Metrics[];
  dockingDiagnostics?: { enabled: boolean };
  status: "offline" | "starting" | "ready";
  settings: Settings;
  document: {
    conversations?: ConversationIndex;
    messages: Message[];
    detached: string[];
    generating: boolean;
    layout: Layout;
    workspaceGroups: WorkspaceGroup[];
    workspaceHosts: Record<string, string>;
  };
  preparation: Preparation;
  logs: string[];
  architecture: string;
  memoryBytes: number | null;
  memoryTuning: MemoryTuning;
  memoryUsage: MemoryUsageState;
  backend: string;
  endpoint: string;
  root: string;
  health: { model?: string } | null;
};
const defaults: Settings = {
  engine: "strata",
  vllm: {
    root: "",
    python: "python3",
    model: "",
    backend: "mlx_lm",
    executable: "",
    evidence: "",
    context: 1024,
    kvCacheMiB: 0,
    allocatorCacheMiB: 0,
  },
  strataRoot: "",
  python: "python3",
  config: "",
  mock: false,
  temperature: 0.7,
  maxTokens: 512,
  language: "ja",
  chatFontSize: 13,
  theme: "dark",
  modelDirectories: [],
  metalMemory: defaultMemory,
};
const panelView = new URLSearchParams(location.search).get("panel");
function App() {
  const [conversationsOpen, setConversationsOpen] = useState(false);
  const [modelsOpen, setModelsOpen] = useState(false);
  const [setupLocalView, setSetupLocalView] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [preferenceTab, setPreferenceTab] = useState<PreferenceTab>("general");
  const [aboutOpen, setAboutOpen] = useState(false);

  const editor = useRef<{
    element: HTMLInputElement | HTMLTextAreaElement;
    start: number | null;
    end: number | null;
  } | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot>();
  const [settings, setSettings] = useState<Settings>(defaults);
  const [language, setLanguage] = useState<Language>("ja");
  const [error, setError] = useState("");
  const [online, setOnline] = useState(false);
  const [connectionFailed, setConnectionFailed] = useState(false);
  const [pending, setPending] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState("");
  const controller = useRef<AbortController | null>(null);
  const busyRef = useRef(false);
  const initialized = useRef(false);
  const conversationEpoch = useRef(0);
  const conversationChanging = useRef(false);
  const draftRef = useRef(settings);
  const dirty = useRef(false);
  const scroll = useRef<HTMLDivElement>(null);
  const followOutput = useRef(true);
  const t = words[language];
  const layout = snapshot?.document.layout || defaultLayout;
  const workspaceTab = layout.workspaceActive;
  const groups = snapshot?.document.workspaceGroups || defaultGroups;
  const workspaceWindow =
    panelView === "workspace" ||
    panelView === "group-chat" ||
    panelView === "group-logs";
  const windowGroup = workspaceWindow ? panelView : null;
  const selectedGroup =
    groups.find((group) => group.tabs.includes(workspaceTab)) || groups[0];
  const hosts = snapshot?.document.workspaceHosts || {};
  const hostedGroups = groups.filter(
    (group) => hosts[group.id] === windowGroup,
  );
  const currentGroup = windowGroup
    ? (
        hostedGroups.find((group) => group.tabs.includes(workspaceTab)) ||
        hostedGroups[0] ||
        selectedGroup
      ).id
    : selectedGroup.id;
  const detached = snapshot?.document.detached || [];
  const groupDetached = detached.includes(currentGroup);
  const structureDisabled =
    !online ||
    pending ||
    (!desktop &&
      detached.some((id) => id === "workspace" || id.startsWith("group-")));
  function setWorkspaceTab(tab: WorkspaceTab) {
    void run(async () => {
      await action("workspaceTab", { tab });
      if (desktop) {
        const target = groups.find((group) => group.tabs.includes(tab));
        if (
          target &&
          ((windowGroup && hosts[target.id] !== windowGroup) ||
            detached.includes(target.id))
        )
          await invoke("focus_workspace_tab", { tab });
      }
    }, true);
  }
  function moveWorkspaceTab(tab: WorkspaceTab, before: WorkspaceTab) {
    void run(() => action("moveWorkspaceTab", { tab, before }));
  }
  async function dockWorkspace(group = currentGroup) {
    await run(() => invoke("dock_panel", { panel: group }));
  }
  function dockWorkspaceTab(
    tab: WorkspaceTab,
    target: WorkspaceTab,
    placement: DockPlacement,
  ) {
    void run(() =>
      desktop
        ? invoke("dock_workspace_tab", { tab, target, placement })
        : action("dockWorkspaceTab", { tab, target, placement }),
    );
  }
  const preparing = ["running", "cancelling"].includes(
    snapshot?.preparation?.status || "",
  );
  useEffect(() => {
    if (!desktop || !snapshot?.dockingDiagnostics?.enabled) return;
    let lastMove = 0;
    function trace(event: Event) {
      const e = event as MouseEvent & DragEvent;
      if (e.type === "pointermove" && !e.buttons) return;
      const time = Date.now();
      if (["pointermove", "dragover"].includes(e.type)) {
        if (time - lastMove < 100) return;
        lastMove = time;
      }
      const element = e.target instanceof Element ? e.target : null;
      void invoke("trace_workspace_drag", {
        event: {
          phase: e.type,
          trusted: e.isTrusted,
          bubbles: e.bubbles,
          x: e.clientX,
          y: e.clientY,
          sx: e.screenX,
          sy: e.screenY,
          buttons: e.buttons,
          button: e.button,
          types: Array.from(e.dataTransfer?.types || []),
          tab: element?.closest('[role="tab"]')?.id,
          target: element?.closest(".dock-target")?.className,
          group: element?.closest("[data-group]")?.getAttribute("data-group"),
          element:
            element?.tagName + "." + (element?.getAttribute("class") || ""),
          wx: window.screenX,
          wy: window.screenY,
          width: innerWidth,
          height: innerHeight,
          time,
        },
      }).catch(() => {});
    }
    const events = [
      "pointerdown",
      "pointermove",
      "pointerup",
      "dragstart",
      "dragenter",
      "dragover",
      "dragleave",
      "drop",
      "dragend",
    ];
    events.forEach((name) => document.addEventListener(name, trace, true));
    return () =>
      events.forEach((name) => document.removeEventListener(name, trace, true));
  }, [snapshot?.dockingDiagnostics?.enabled]);
  useEffect(() => {
    if (!desktop) return;
    const unlisten = listen("workspace-cancel-generation", () =>
      controller.current?.abort(),
    );
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  function cancelGeneration() {
    if (controller.current) controller.current.abort();
    else if (desktop)
      void emit("workspace-cancel-generation").catch((e) =>
        setError(String(e)),
      );
  }
  useEffect(() => {
    document.documentElement.lang = language === "zh" ? "zh-CN" : language;
  }, [language]);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        if (!initialized.current) {
          await connect();
        }
        const epoch = conversationEpoch.current;
        const s: Snapshot = await request("/studio/state").then((r) =>
          r.json(),
        );
        if (!alive) return;
        if (
          conversationChanging.current ||
          epoch !== conversationEpoch.current
        ) {
          timer = setTimeout(poll, 1200);
          return;
        }
        setSnapshot(s);
        setOnline(true);
        setConnectionFailed(false);
        setLanguage(s.settings.language);
        if (!initialized.current || !dirty.current) {
          setSettings(s.settings);
          draftRef.current = s.settings;
          initialized.current = true;
        }
        if (!busyRef.current || s.document.generating)
          setMessages(s.document.messages);
      } catch {
        if (alive) {
          setOnline(false);
          setConnectionFailed(true);
          initialized.current = false;
        }
      }
      if (alive) timer = setTimeout(poll, 1200);
    }
    poll();
    return () => {
      alive = false;
      clearTimeout(timer);
      controller.current?.abort();
    };
  }, []);
  useEffect(() => {
    if (followOutput.current)
      scroll.current?.scrollTo({ top: scroll.current.scrollHeight });
  }, [
    messages.length,
    messages.at(-1)?.content,
    messages.at(-1)?.inference?.reasoning,
    snapshot?.document.conversations?.activeId,
  ]);
  useEffect(() => {
    document.documentElement.dataset.theme = settings.theme;
    document.documentElement.style.setProperty(
      "--chat-font-size",
      `${settings.chatFontSize}px`,
    );
    if (desktop)
      void getCurrentWindow().setTheme(settings.theme).catch(console.error);
  }, [settings.theme, settings.chatFontSize]);
  const [updateInfo, setUpdateInfo] = useState<{ status: string; current: string; latest?: string } | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const updateWords = {
    en: { check: "Check for updates", checking: "Checking…", available: "Update available", current: "Up to date", noRelease: "No release available or repository inaccessible", open: "Open releases", error: "Could not check for updates" },
    ja: { check: "アップデートを確認", checking: "確認中…", available: "アップデートがあります", current: "最新です", noRelease: "リリースがないか、リポジトリにアクセスできません", open: "リリースを開く", error: "アップデートを確認できません" },
    zh: { check: "检查更新", checking: "正在检查…", available: "有可用更新", current: "已是最新版本", noRelease: "没有可用版本或无法访问仓库", open: "打开发布页面", error: "无法检查更新" },
  }[language];
  async function saveAppearance(
    value: Partial<Pick<Settings, "theme" | "chatFontSize">>,
  ) {
    await action("appearance", value);
    draftRef.current = { ...draftRef.current, ...value };
    setSettings((current) => ({ ...current, ...value }));
  }
  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    const next = { ...draftRef.current, [key]: value };
    dirty.current = true;
    draftRef.current = next;
    setSettings(next);
  }
  async function persist() {
    await action("settings", { settings: draftRef.current });
    dirty.current = false;
  }
  async function run(fn: () => Promise<unknown>, quiet = false) {
    setError("");
    if (!quiet) setPending(true);
    try {
      await fn();
      const s = await request("/studio/state").then((r) => r.json());
      setSnapshot(s);
    } catch (e) {
      setError(String((e as Error).message));
    } finally {
      if (!quiet) setPending(false);
    }
  }
  async function changeLanguage(l: Language) {
    setLanguage(l);
    update("language", l);
    await run(persist);
  }
  async function inspectSetup() {
    const data: Inspection = await request("/studio/inspect").then((r) =>
      r.json(),
    );
    setInspection(data);
  }
  async function browse(kind: "root" | "config" | "python") {
    await run(async () => {
      const path = await invoke<string | null>("select_path", { kind });
      if (path) {
        update(kind === "root" ? "strataRoot" : kind, path);
        await persist();
      }
    });
  }
  async function openSetup() {
    setSetupLocalView(false);
    await run(async () => {
      if (!preparing) await persist();
      await inspectSetup();
      setSetupOpen(true);
    });
  }
  async function float(panel: string) {
    if (!desktop) {
      setError(t.unavailable);
      return;
    }
    await run(() =>
      invoke(panelView ? "dock_panel" : "float_panel", { panel }),
    );
  }
  async function copy(value: string, id: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(id);
      setTimeout(() => setCopied(""), 1500);
    } catch (e) {
      setError(String(e));
    }
  }
  async function send() {
    if (
      !prompt.trim() ||
      busy ||
      snapshot?.document.generating ||
      snapshot?.status !== "ready"
    )
      return;
    setError("");
    followOutput.current = true;
    const next = [...messages, { role: "user", content: prompt.trim() }];
    setPrompt("");
    busyRef.current = true;
    setBusy(true);
    const c = new AbortController();
    controller.current = c;
    let answer = "";
    setMessages([...next, { role: "assistant", content: "" }]);
    try {
      await persist();
      await streamChat(
        {
          conversationId: snapshot.document.conversations?.activeId,
          model:
            settings.engine === "vllmApple"
              ? settings.vllm.backend === "mlx_lm"
                ? "default_model"
                : settings.vllm.model
              : snapshot.health?.model || "qwen3.8-flash-next",
          messages: next,
          temperature: settings.temperature,
          max_tokens: settings.maxTokens,
          stream: true,
        },
        c.signal,
        (delta) => {
          answer += delta;
          setMessages((current) => [
            ...next,
            {
              role: "assistant",
              content: answer,
              inference: current.at(-1)?.inference,
            },
          ]);
        },
      );
    } catch (e) {
      if ((e as Error).name !== "AbortError") setError((e as Error).message);
    } finally {
      try {
        const authoritative: Snapshot = await request("/studio/state").then(
          (r) => r.json(),
        );
        setSnapshot(authoritative);
        setMessages(authoritative.document.messages);
      } catch {
        /* The next successful poll restores the Rust-owned response. */
      }
      busyRef.current = false;
      setBusy(false);
      controller.current = null;
    }
  }
  const status = online ? snapshot?.status || "offline" : "offline";
  function button(
    icon: React.ReactNode,
    title: string,
    onClick: () => void,
    disabled = false,
  ) {
    return (
      <button
        className="icon-button"
        title={title}
        aria-label={title}
        onClick={onClick}
        disabled={disabled}
      >
        {icon}
      </button>
    );
  }
  function panelHeader(
    id: "engine" | "api" | "logs",
    icon: React.ReactNode,
    extra?: React.ReactNode,
  ) {
    return (
      <header
        className="panel-header"
        draggable={desktop && !!panelView}
        onDragStart={(e) => e.dataTransfer.setData("text/plain", id)}
      >
        <h2>
          {icon}
          {t[id]}
        </h2>
        <div className="tools">
          {extra}
          {panelView &&
            !workspaceWindow &&
            button(
              panelView ? <PanelLeftClose /> : <ExternalLink />,
              panelView ? t.dock : t.float,
              () => float(id),
            )}
        </div>
      </header>
    );
  }
  function engine() {
    return (
      <section className="panel engine">
        {panelHeader(
          "engine",
          <Cpu />,
          <>
            <div className="process-buttons">
              <button
                className="primary"
                disabled={
                  pending || !online || status !== "offline" || preparing
                }
                onClick={() =>
                  run(async () => {
                    await persist();
                    await action("start");
                  })
                }
              >
                <Play size={13} />
                {t.start}
              </button>
              <button
                disabled={pending || !online || status === "offline" || busy}
                onClick={() => run(() => action("stop"))}
              >
                <Square size={11} />
                {t.stop}
              </button>
            </div>
            {settings.engine === "vllmApple" &&
              button(
                <Info />,
                t.vllmInspect,
                () =>
                  run(async () => {
                    if (dirty.current) await persist();
                    await action("inspectVllm");
                  }),
                !online || pending || status !== "offline" || preparing,
              )}
            {settings.engine === "strata" && (
              <>
                {button(
                  <FolderSearch />,
                  t.localModels,
                  () =>
                    run(async () => {
                      if (dirty.current && !preparing) await persist();
                      setModelsOpen(true);
                    }),
                  !online || pending || settings.engine !== "strata",
                )}
                {button(
                  <Download />,
                  preparing ? t.setupRunning : t.prepare,
                  () => openSetup(),
                  !online || pending || settings.engine !== "strata",
                )}
              </>
            )}
          </>,
        )}
        <div className="panel-body">
          <label>
            {t.engineSelector}
            <span title={t.engineSwitchHelp}>
              <Info size={13} />
            </span>
            <select
              aria-label={t.engineSelector}
              value={settings.engine}
              disabled={
                !online ||
                pending ||
                preparing ||
                busy ||
                !!snapshot?.document.generating
              }
              onChange={(e) => {
                const engine = e.target.value as Settings["engine"];
                void run(async () => {
                  if (dirty.current) await persist();
                  await action("switchEngine", { engine });
                  const state: Snapshot = await request("/studio/state").then(
                    (r) => r.json(),
                  );
                  setSettings(state.settings);
                  draftRef.current = state.settings;
                  dirty.current = false;
                });
              }}
            >
              <option value="strata">Strata</option>
              <option value="vllmApple">vLLM-Apple</option>
            </select>
          </label>
          {settings.engine === "strata" ? (
            <>
              {snapshot?.backend === "Metal" && (
                <MemoryControls
                  value={settings.metalMemory ?? defaultMemory}
                  tuning={snapshot?.memoryTuning}
                  usage={snapshot?.memoryUsage}
                  memoryBytes={snapshot?.memoryBytes ?? null}
                  language={language}
                  disabled={preparing || pending || !online}
                  running={status !== "offline"}
                  generating={busy || !!snapshot?.document.generating}
                  onRestart={async (value) => {
                    await action("applyMemoryRestart", { memory: value });
                    const next = { ...draftRef.current, metalMemory: value };
                    draftRef.current = next;
                    setSettings(next);
                  }}
                  onSave={async (value) => {
                    update("metalMemory", value);
                    await persist();
                  }}
                />
              )}
              <label>
                {t.root}
                <div className="path-input">
                  <input
                    aria-label={t.root}
                    value={settings.strataRoot}
                    disabled={status !== "offline" || preparing}
                    onChange={(e) => update("strataRoot", e.target.value)}
                    onBlur={() =>
                      persist().catch((e: Error) => setError(e.message))
                    }
                  />
                  {desktop
                    ? button(
                        <FolderOpen />,
                        t.browse + " · " + t.root,
                        () => browse("root"),
                        status !== "offline" || preparing || pending,
                      )
                    : null}
                </div>
              </label>
              <label>
                {t.config}
                <span title={t.configHelp}>
                  <Info size={13} />
                </span>
                <div className="path-input">
                  <input
                    aria-label={t.config}
                    value={settings.config}
                    placeholder="strata-iq3_s.json"
                    disabled={status !== "offline" || preparing}
                    onBlur={() =>
                      persist().catch((e: Error) => setError(e.message))
                    }
                    onChange={(e) => update("config", e.target.value)}
                  />
                  {desktop
                    ? button(
                        <FolderOpen />,
                        t.browse + " · " + t.config,
                        () => browse("config"),
                        status !== "offline" || preparing || pending,
                      )
                    : null}
                </div>
              </label>
              <label>
                {t.python}
                <div className="path-input">
                  <input
                    aria-label={t.python}
                    value={settings.python}
                    disabled={status !== "offline" || preparing}
                    onBlur={() =>
                      persist().catch((e: Error) => setError(e.message))
                    }
                    onChange={(e) => update("python", e.target.value)}
                  />
                  {desktop
                    ? button(
                        <FolderOpen />,
                        t.browse + " · " + t.python,
                        () => browse("python"),
                        status !== "offline" || preparing || pending,
                      )
                    : null}
                </div>
              </label>
            </>
          ) : (
            <>
              <label>
                {t.vllmBackend}
                <select
                  aria-label={t.vllmBackend}
                  value={settings.vllm.backend}
                  disabled={status !== "offline" || pending}
                  onChange={(e) => {
                    update("vllm", {
                      ...settings.vllm,
                      backend: e.target.value as Settings["vllm"]["backend"],
                      allocatorCacheMiB:
                        e.target.value === "mlx_lm"
                          ? settings.vllm.allocatorCacheMiB
                          : 0,
                      kvCacheMiB:
                        e.target.value === "mlx_lm"
                          ? settings.vllm.kvCacheMiB
                          : 0,
                    });
                    void run(persist);
                  }}
                >
                  <option value="mlx_lm">MLX LM</option>
                  <option value="vllm_metal">vLLM-Metal</option>
                </select>
              </label>
              {!dirty.current && snapshot?.vllmInspection && (
                <VllmInspectionView
                  value={snapshot.vllmInspection}
                  language={language}
                />
              )}
              {(
                [
                  ["root", t.vllmRoot, "root"],
                  ["python", t.python, "python"],
                  ["model", t.vllmModel, "models"],
                  ["executable", t.vllmExecutable, "python"],
                  ["evidence", t.vllmEvidence, "evidence"],
                ] as const
              ).map(([key, label, kind]) => (
                <label key={key}>
                  {label}
                  {key === "evidence" && (
                    <span title={t.vllmEvidenceHelp} tabIndex={0}>
                      <Info size={13} />
                    </span>
                  )}
                  <div className="path-input">
                    <input
                      aria-label={label}
                      value={settings.vllm[key]}
                      disabled={status !== "offline" || pending}
                      onChange={(e) =>
                        update("vllm", {
                          ...draftRef.current.vllm,
                          [key]: e.target.value,
                          ...(key === "evidence" && !e.target.value.trim()
                            ? { kvCacheMiB: 0, allocatorCacheMiB: 0 }
                            : {}),
                        })
                      }
                      onBlur={() => {
                        void persist().catch((e: Error) => setError(e.message));
                      }}
                    />
                    {desktop
                      ? button(
                          <FolderOpen />,
                          t.browse + " · " + label,
                          () => {
                            void run(async () => {
                              const path = await invoke<string | null>(
                                "select_path",
                                { kind },
                              );
                              if (path) {
                                update("vllm", {
                                  ...draftRef.current.vllm,
                                  [key]: path,
                                });
                                await persist();
                              }
                            });
                          },
                          status !== "offline" || pending,
                        )
                      : null}
                  </div>
                </label>
              ))}
              <label>
                {t.contextLength}
                <input
                  type="number"
                  aria-label={t.contextLength}
                  min={1}
                  max={131072}
                  value={
                    Number.isFinite(settings.vllm.context)
                      ? settings.vllm.context
                      : ""
                  }
                  disabled={status !== "offline" || pending}
                  onChange={(e) =>
                    update("vllm", {
                      ...draftRef.current.vllm,
                      context: e.target.valueAsNumber,
                    })
                  }
                  onBlur={() => {
                    void persist().catch((e: Error) => setError(e.message));
                  }}
                />
              </label>
              <label>
                {t.vllmKvCache}
                <span title={t.vllmKvCacheHelp} tabIndex={0}>
                  <Info size={13} />
                </span>
                <input
                  type="number"
                  aria-label={t.vllmKvCache}
                  min={0}
                  max={4096}
                  step={1}
                  value={
                    Number.isFinite(settings.vllm.kvCacheMiB)
                      ? settings.vllm.kvCacheMiB
                      : ""
                  }
                  disabled={
                    status !== "offline" ||
                    pending ||
                    !settings.vllm.evidence ||
                    settings.vllm.backend !== "mlx_lm"
                  }
                  onChange={(e) =>
                    update("vllm", {
                      ...draftRef.current.vllm,
                      kvCacheMiB: e.target.valueAsNumber,
                      ...(e.target.valueAsNumber === 0
                        ? { allocatorCacheMiB: 0 }
                        : {}),
                    })
                  }
                  onBlur={() => {
                    void persist().catch((e: Error) => setError(e.message));
                  }}
                />
              </label>
              <label>
                {t.vllmAllocatorCache}
                <span title={t.vllmAllocatorCacheHelp} tabIndex={0}>
                  <Info size={13} />
                </span>
                <input
                  type="number"
                  aria-label={t.vllmAllocatorCache}
                  min={0}
                  max={4096}
                  step={1}
                  value={
                    Number.isFinite(settings.vllm.allocatorCacheMiB)
                      ? settings.vllm.allocatorCacheMiB
                      : ""
                  }
                  disabled={
                    status !== "offline" ||
                    pending ||
                    !settings.vllm.evidence ||
                    !settings.vllm.kvCacheMiB ||
                    settings.vllm.backend !== "mlx_lm"
                  }
                  onChange={(e) =>
                    update("vllm", {
                      ...draftRef.current.vllm,
                      allocatorCacheMiB: e.target.valueAsNumber,
                    })
                  }
                  onBlur={() => {
                    void persist().catch((e: Error) => setError(e.message));
                  }}
                />
              </label>
              <MemoryUsage usage={snapshot?.memoryUsage} language={language} />
              <MemoryHistory
                history={snapshot?.memoryUsage?.history}
                events={snapshot?.memoryUsage?.events}
                language={language}
              />
            </>
          )}
          <div className="value-row">
            <span>{t.port}</span>
            <code>8080</code>
          </div>
          <div className="value-row">
            <span>{t.platform}</span>
            <code>
              {snapshot?.architecture || "—"} / {snapshot?.backend || "—"}
            </code>
          </div>
          {settings.engine === "strata" && (
            <label className="toggle">
              <input
                type="checkbox"
                checked={settings.mock}
                disabled={status !== "offline" || preparing}
                onChange={(e) => {
                  update("mock", e.target.checked);
                  run(persist);
                }}
              />
              {t.mock}
              <span title={t.mockHelp}>
                <Info size={13} />
              </span>
            </label>
          )}
          <div className="divider" />
          <div className="value-row">
            <span>{t.status}</span>
            <span className={"status " + status}>
              <i />
              {t[status]}
            </span>
          </div>
          {preparing ? (
            <p className="test-label" role="status">
              {t.setupRunning}
            </p>
          ) : null}
          {settings.engine === "strata" &&
          settings.mock &&
          status !== "offline" ? (
            <p className="test-label">{t.testMode}</p>
          ) : null}
        </div>
      </section>
    );
  }
  function api() {
    return (
      <section className="panel api">
        {panelHeader("api", <Link />)}
        <div className="panel-body">
          <label>
            {t.base}
            <div className="copy-field">
              <code>{snapshot?.endpoint || "http://127.0.0.1:8080/v1"}</code>
              {button(copied === "url" ? <Check /> : <Copy />, t.copy, () =>
                copy(snapshot?.endpoint || "", "url"),
              )}
            </div>
          </label>
          <label>
            {t.key}
            <div className="copy-field">
              <code>••••••••••••••••</code>
              {button(copied === "key" ? <Check /> : <Copy />, t.copy, () =>
                copy(token(), "key"),
              )}
            </div>
          </label>
          <div className="divider" />
          <h3>{t.endpoints}</h3>
          <div className="endpoint">
            <span>OpenAI</span>
            <code>/v1/chat/completions</code>
          </div>
          {settings.engine === "strata" && (
            <div className="endpoint">
              <span>Anthropic</span>
              <code>/v1/messages</code>
            </div>
          )}
          <div className="endpoint">
            <span>Models</span>
            <code>/v1/models</code>
          </div>
          <div className="divider" />
          <h3>{t.request}</h3>
          <label className="value-row">
            {t.temperature}
            <input
              disabled={preparing}
              aria-label={t.temperature}
              type="number"
              min="0"
              max="2"
              step="0.1"
              value={
                Number.isFinite(settings.temperature)
                  ? settings.temperature
                  : ""
              }
              onChange={(e) => update("temperature", e.target.valueAsNumber)}
              onBlur={() => persist().catch((e: Error) => setError(e.message))}
            />
          </label>
          <label className="value-row">
            {t.tokens}
            <input
              disabled={preparing}
              aria-label={t.tokens}
              type="number"
              min="1"
              max="262144"
              value={
                Number.isFinite(settings.maxTokens) ? settings.maxTokens : ""
              }
              onChange={(e) => update("maxTokens", e.target.valueAsNumber)}
              onBlur={() => persist().catch((e: Error) => setError(e.message))}
            />
          </label>
          <div className="model-name">
            <Layers size={15} />
            <code>{snapshot?.health?.model || "—"}</code>
          </div>
          <RequestLog requests={snapshot?.requests} language={language} />
        </div>
      </section>
    );
  }
  function logs() {
    return (
      <section className="panel logs">
        {panelHeader(
          "logs",
          <FileText />,
          button(
            <Trash2 />,
            t.clear,
            () => run(() => action("clearLogs")),
            !online,
          ),
        )}
        <div className="log-lines">
          {snapshot?.logs.length ? (
            snapshot.logs.map((line, i) => (
              <div key={i}>
                <span>{i + 1}</span>
                <code>{line}</code>
              </div>
            ))
          ) : (
            <div>
              <span>1</span>
              <code>{t.none}</code>
            </div>
          )}
        </div>
      </section>
    );
  }
  function chat() {
    return (
      <section className="panel chat">
        <header className="panel-header">
          <h2>
            <MessageSquare />
            {t.chat}
          </h2>
          {button(
            <History />,
            t.conversationLibrary,
            () => setConversationsOpen(true),
            !online,
          )}
          {button(
            <Plus />,
            t.newChat,
            newChat,
            busy || snapshot?.document.generating || pending || !online,
          )}
        </header>
        <div
          className="conversation"
          ref={scroll}
          onScroll={(e) => {
            const node = e.currentTarget;
            followOutput.current =
              node.scrollHeight - node.scrollTop - node.clientHeight < 72;
          }}
        >
          {messages.length ? (
            messages.map((m, i) => (
              <article className={"message " + m.role} key={i}>
                <div>{m.role === "user" ? t.you : "Soliton"}</div>
                {m.role === "assistant" && (
                  <InferenceView value={m.inference} language={language} />
                )}
                {m.content ? (
                  <p>{m.content}</p>
                ) : !m.inference || m.inference.metrics.status === "running" ? (
                  <p>…</p>
                ) : null}
                {m.role === "assistant" && m.inference && (
                  <RequestMetrics
                    value={m.inference.metrics}
                    language={language}
                  />
                )}
              </article>
            ))
          ) : (
            <div className="empty">
              <MessageSquare />
              <p>{t.empty}</p>
            </div>
          )}
        </div>
        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault();
            send();
          }}
        >
          <textarea
            aria-label={t.placeholder}
            placeholder={t.placeholder}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => {
              if (
                e.key === "Enter" &&
                !e.shiftKey &&
                !e.nativeEvent.isComposing
              ) {
                e.preventDefault();
                send();
              }
            }}
          />
          {busy || snapshot?.document.generating ? (
            <button
              type="button"
              className="primary"
              aria-label={t.cancel}
              title={t.cancel}
              onClick={cancelGeneration}
            >
              <Square />
            </button>
          ) : (
            <button
              className="primary"
              aria-label={t.send}
              title={t.send}
              disabled={
                status !== "ready" ||
                snapshot?.document.generating ||
                !prompt.trim()
              }
            >
              <Send />
            </button>
          )}
        </form>
      </section>
    );
  }
  function openPreferences(tab: PreferenceTab = "general") {
    setPreferenceTab(tab);
    setPreferencesOpen(true);
  }
  async function closePreferences() {
    try {
      if (dirty.current && !preparing) await persist();
      setPreferencesOpen(false);
    } catch (e) {
      setError(String((e as Error).message));
    }
  }
  function captureEditor() {
    const element = document.activeElement;
    if (
      (element instanceof HTMLTextAreaElement ||
        element instanceof HTMLInputElement) &&
      !element.readOnly &&
      !element.disabled
    ) {
      editor.current = {
        element,
        start: element.selectionStart,
        end: element.selectionEnd,
      };
    }
  }
  async function edit(command: string) {
    if (workspaceTab === "logs" && (!panelView || workspaceWindow)) {
      if (command === "selectAll") {
        const log = document.querySelector(".log-lines");
        if (log) {
          const range = document.createRange();
          range.selectNodeContents(log);
          const selection = window.getSelection();
          selection?.removeAllRanges();
          selection?.addRange(range);
        }
      } else if (command === "copy") {
        await copy(window.getSelection()?.toString() || "", "logs");
      }
      return;
    }

    const saved = editor.current;
    const target = saved?.element.isConnected
      ? saved.element
      : document.querySelector<HTMLTextAreaElement>(".composer textarea");
    if (!target) return;
    target.focus();
    if (saved?.element === target && saved.start !== null && saved.end !== null)
      target.setSelectionRange(saved.start, saved.end);
    try {
      if (command === "paste") {
        const text = await navigator.clipboard.readText();
        document.execCommand("insertText", false, text);
      } else document.execCommand(command);
      captureEditor();
    } catch (e) {
      setError(String((e as Error).message));
    }
  }
  async function exportChat() {
    if (desktop) {
      await run(() => invoke("export_conversation"));
      return;
    }
    const url = URL.createObjectURL(
      new Blob([JSON.stringify({ version: 1, messages }, null, 2)], {
        type: "application/json",
      }),
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = "Soliton-conversation.json";
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  async function conversationAction(name: string, id?: string) {
    followOutput.current = true;
    conversationChanging.current = true;
    conversationEpoch.current++;
    try {
      await action(name, { id });
      const next: Snapshot = await request("/studio/state").then((r) =>
        r.json(),
      );
      setSnapshot(next);
      setMessages(next.document.messages);
      if (
        name !== "deleteConversation" ||
        next.document.conversations?.activeId !==
          snapshot?.document.conversations?.activeId
      )
        setPrompt("");
      setWorkspaceTab("chat");
    } finally {
      conversationEpoch.current++;
      conversationChanging.current = false;
    }
  }
  const newChat = () => run(() => conversationAction("newConversation"));
  const menus: AppMenu[] = [
    {
      label: "Soliton",
      icon: <Layers />,
      brand: true,
      items: [
        { label: t.about, action: () => setAboutOpen(true) },
        {
          label: t.preferences,
          shortcut: "⌘,",
          action: () => openPreferences(),
          separator: true,
        },
        {
          label: t.quit,
          shortcut: "⌘Q",
          action: () => run(() => invoke("quit_app")),
          disabled: !desktop,
          separator: true,
        },
      ],
    },
    {
      label: t.fileMenu,
      items: [
        {
          label: t.newChat,
          shortcut: "⌘N",
          action: newChat,
          disabled: !online || busy || snapshot?.document.generating,
        },
        {
          label: t.conversationLibrary,
          action: () => setConversationsOpen(true),
          disabled: !online,
        },
        {
          label: t.exportChat,
          action: exportChat,
          disabled: !messages.length || busy,
        },
        {
          label: t.preferences,
          shortcut: "⌘,",
          action: () => openPreferences(),
          separator: true,
        },
      ],
    },
    {
      label: t.editMenu,
      items: [
        {
          label: t.undo,
          shortcut: "⌘Z",
          action: () => edit("undo"),
          disabled: workspaceTab !== "chat",
        },
        {
          label: t.redo,
          shortcut: "⇧⌘Z",
          action: () => edit("redo"),
          disabled: workspaceTab !== "chat",
        },
        {
          label: t.cut,
          shortcut: "⌘X",
          action: () => edit("cut"),
          disabled: workspaceTab !== "chat",
          separator: true,
        },
        { label: t.copy, shortcut: "⌘C", action: () => edit("copy") },
        {
          label: t.paste,
          shortcut: "⌘V",
          action: () => edit("paste"),
          disabled: workspaceTab !== "chat",
        },
        {
          label: t.selectAll,
          shortcut: "⌘A",
          action: () => edit("selectAll"),
          separator: true,
        },
      ],
    },
    {
      label: t.viewMenu,
      items: [
        {
          label: t.chat,
          shortcut: "⌘1",
          selected: workspaceTab === "chat",
          action: () => setWorkspaceTab("chat"),
        },
        {
          label: t.logs,
          shortcut: "⌘2",
          selected: workspaceTab === "logs",
          action: () => setWorkspaceTab("logs"),
        },
        {
          label:
            groupDetached && !workspaceWindow
              ? t.dockGroup
              : workspaceWindow
                ? t.dockGroup
                : t.floatGroup,
          disabled: !desktop || pending || !online,
          action: () =>
            groupDetached || workspaceWindow
              ? dockWorkspace()
              : float(currentGroup),
          separator: true,
        },
        {
          label: t.resetWorkspace,
          disabled:
            structureDisabled ||
            detached.some(
              (id) => id === "workspace" || id.startsWith("group-"),
            ),
          action: () => run(() => action("resetWorkspace")),
        },
      ],
    },
    {
      label: t.helpMenu,
      items: [{ label: t.about, action: () => setAboutOpen(true) }],
    },
  ];
  useEffect(() => {
    function shortcuts(e: KeyboardEvent) {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.isComposing) return;
      if (document.querySelector("dialog[open]")) return;
      if (e.key === ",") {
        e.preventDefault();
        openPreferences();
      } else if (
        e.key.toLowerCase() === "n" &&
        online &&
        !busy &&
        !snapshot?.document.generating
      ) {
        e.preventDefault();
        newChat();
      } else if (["1", "2"].includes(e.key)) {
        e.preventDefault();
        setWorkspaceTab(e.key === "1" ? "chat" : "logs");
      }
    }
    document.addEventListener("keydown", shortcuts);
    return () => document.removeEventListener("keydown", shortcuts);
  });
  return (
    <div className={"workbench app-shell " + (panelView ? "floating" : "")}>
      <header className="app-header">
        <AppMenus menus={menus} onBeforeOpen={captureEditor} />
        {button(<SettingsIcon />, t.preferences, () => openPreferences())}
      </header>
      {!online ? (
        <div className="notice" role="status">
          {connectionFailed ? t.connectionError : t.connecting}
        </div>
      ) : null}
      {error ? (
        <div className="notice error" role="alert">
          {error}
          {button(<Square />, t.clear, () => setError(""))}
        </div>
      ) : null}
      {panelView && !workspaceWindow ? (
        <main className="floating-main">
          {panelView === "engine"
            ? engine()
            : panelView === "api"
              ? api()
              : logs()}
        </main>
      ) : (
        <main className="workspace-host">
          <WorkspaceDock
            groups={groups}
            trace={snapshot?.dockingDiagnostics?.enabled}
            layout={layout}
            detached={detached}
            hosts={hosts}
            windowGroup={windowGroup}
            language={language}
            native={desktop}
            disabled={!online || pending}
            structureDisabled={structureDisabled}
            onSelect={setWorkspaceTab}
            onMove={moveWorkspaceTab}
            onFloat={(group) => float(group)}
            onDockWindow={(group) => dockWorkspace(group)}
            onDockTab={dockWorkspaceTab}
            onResize={(ratio) =>
              run(() => action("resizeWorkspace", { ratio }))
            }
            render={(tab) => (tab === "chat" ? chat() : logs())}
          />
        </main>
      )}
      {preferencesOpen && (
        <PreferencesDialog
          language={language}
          tab={preferenceTab}
          onTab={setPreferenceTab}
          onClose={closePreferences}
          error={error}
        >
          {preferenceTab === "general" ? (
            <section className="general-settings">
              <label>{t.appearanceTheme}</label>
              <div
                className="appearance-options"
                role="group"
                aria-label={t.appearanceTheme}
              >
                {(["light", "dark"] as const).map((theme) => (
                  <button
                    key={theme}
                    type="button"
                    aria-pressed={settings.theme === theme}
                    disabled={!online || pending}
                    onClick={() => run(() => saveAppearance({ theme }))}
                  >
                    {theme === "light" ? <Sun size={15} /> : <Moon size={15} />}{" "}
                    {theme === "light" ? t.lightMode : t.darkMode}
                  </button>
                ))}
              </div>
              <label className="chat-font-setting">
                {t.chatFontSize}
                <select
                  aria-label={t.chatFontSize}
                  value={settings.chatFontSize}
                  disabled={!online || pending}
                  onChange={(e) =>
                    run(() =>
                      saveAppearance({ chatFontSize: Number(e.target.value) }),
                    )
                  }
                >
                  {Array.from({ length: 21 }, (_, i) => i + 12).map((size) => (
                    <option key={size} value={size}>
                      {size} px
                    </option>
                  ))}
                </select>
              </label>
              <p
                className="appearance-preview"
                style={{ fontSize: settings.chatFontSize }}
              >
                {t.fontPreview}
              </p>
              {desktop && <div className="software-update">
                <button type="button" disabled={updateBusy} onClick={async () => {
                  setUpdateBusy(true);
                  try { setUpdateInfo(await invoke("check_software_update")); }
                  catch { setUpdateInfo({ status: "error", current: "" }); }
                  finally { setUpdateBusy(false); }
                }}>{updateBusy ? updateWords.checking : updateWords.check}</button>
                {updateInfo && <p role="status">{updateInfo.status === "available" ? updateWords.available : updateInfo.status === "current" ? updateWords.current : updateInfo.status === "noRelease" ? updateWords.noRelease : updateWords.error}{updateInfo.latest ? ` · ${updateInfo.latest}` : ""}</p>}
                {updateInfo?.status === "available" && <button type="button" onClick={() => void invoke("open_releases_page")}>{updateWords.open}</button>}
              </div>}
              <label>{t.language}</label>
              <nav className="language-options" aria-label={t.language}>
                {(["ja", "en", "zh"] as const).map((l) => (
                  <button
                    key={l}
                    aria-pressed={l === language}
                    disabled={!online || preparing}
                    onClick={() => changeLanguage(l)}
                  >
                    {l === "ja"
                      ? "日本語"
                      : l === "en"
                        ? "English"
                        : "简体中文"}
                  </button>
                ))}
              </nav>
            </section>
          ) : preferenceTab === "engine" ? (
            engine()
          ) : (
            api()
          )}
        </PreferencesDialog>
      )}
      {modelsOpen && (
        <ModelLibraryDialog
          language={language}
          directories={settings.modelDirectories || []}
          locked={preparing}
          engineOffline={status === "offline"}
          onUse={async (path) => {
            if (dirty.current) await persist();
            await action("useLocalModel", { path, confirmed: true });
            const next: Snapshot = await request("/studio/state").then((r) =>
              r.json(),
            );
            setSnapshot(next);
            setSettings(next.settings);
            draftRef.current = next.settings;
            dirty.current = false;
            setModelsOpen(false);
            if (["running", "cancelling"].includes(next.preparation.status)) {
              await inspectSetup();
              setSetupLocalView(true);
              setSetupOpen(true);
            }
          }}
          onDirectories={async (paths) => {
            update("modelDirectories", paths);
            await persist();
          }}
          onClose={() => setModelsOpen(false)}
        />
      )}
      {conversationsOpen && (
        <ConversationLibrary
          library={snapshot?.document.conversations}
          language={language}
          disabled={
            busy || !!snapshot?.document.generating || pending || !online
          }
          onAction={conversationAction}
          onClose={() => setConversationsOpen(false)}
        />
      )}
      {aboutOpen && (
        <Modal
          title={t.about}
          language={language}
          className="about-dialog"
          onClose={() => setAboutOpen(false)}
        >
          <div className="about-content">
            <Layers className="about-logo" />
            <h3>Soliton</h3>
            <p>{t.aboutDescription}</p>
            <dl>
              <dt>{t.version}</dt>
              <dd>0.1.0</dd>
              <dt>{t.platform}</dt>
              <dd>
                {snapshot?.architecture || "—"} / {snapshot?.backend || "—"}
              </dd>
            </dl>
            <small>Soliton · MIT License</small>
          </div>
        </Modal>
      )}
      {setupOpen ? (
        <SetupDialog
          language={language}
          localView={setupLocalView}
          inspection={inspection}
          job={
            snapshot?.preparation || { status: "idle", model: "", error: null }
          }
          engineOffline={status === "offline"}
          config={settings.config}
          onModels={() => setModelsOpen(true)}
          onInspect={inspectSetup}
          onConfig={async (path) => {
            update("config", path);
            await persist();
            await inspectSetup();
          }}
          onPrepare={async (model) => {
            await action("prepare", { model, confirmed: true });
            const s = await request("/studio/state").then((r) => r.json());
            setSnapshot(s);
          }}
          onCancel={async () => {
            await action("cancelPreparation");
          }}
          onClose={() => setSetupOpen(false)}
        />
      ) : null}
      <footer className="statusbar">
        <span>Soliton</span>
        <span>
          <Monitor size={14} />
          {snapshot?.architecture || "—"} / {snapshot?.backend || "—"}
        </span>
        <span className={"status " + status}>
          <i />
          {t[status]}
        </span>
      </footer>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
