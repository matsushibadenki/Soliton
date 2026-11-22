import { ChatMath } from "./ChatMath";
import { EngineGuide, type Profile } from "./EngineGuide";
import { ModelPlan, type ModelPlanState } from "./ModelPlan";
import { useEffect, useEffectEvent, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
  Activity,
  Sun,
  Moon,
  PanelLeftOpen,
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
  Paperclip,
  X,
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
import { InstallationRepair } from "./InstallationRepair";
import { ModelLibraryDialog } from "./ModelLibraryDialog";
import {
  MemoryControls,
  defaultMemory,
  type MemorySettings,
  type MemoryTuning,
} from "./MemoryControls";
import { MemoryUsage, type MemoryUsageState } from "./MemoryUsage";
import { MemoryHistory } from "./MemoryHistory";
import { PrefixEngineSettings, defaultPrefixSettings, type PrefixSettings } from "./PrefixEngineSettings";
import { OmniSettings, OmniVoice } from "./OmniVoice";
import { KnowledgeLab } from "./KnowledgeLab";
import { knowledgeWords } from "./knowledge-i18n";
type Settings = {
  engine: "strata" | "vllmApple" | "mlxPrefix" | "lfm3Omni";
  omni: { runtimeDir: string; model: string; projector: string; mediaRoot: string; evidence: string; realtimeEvidence: string; context: number };
  prefixCache: PrefixSettings;
  vllm: {
    root: string;
    python: string;
    model: string;
    backend: "mlx_lm" | "vllm_metal";
    executable: string;
    evidence: string;
    context: number;
    mpsFallback?: boolean | null;
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
const enginePresetKeys = {
  strata: ["strataRoot", "python", "config", "mock", "metalMemory"],
  vllmApple: ["vllm"],
  mlxPrefix: ["prefixCache"],
  lfm3Omni: ["omni"],
} as const;
type EnginePreset = {
  engine: Settings["engine"];
  name: string;
  settings: Partial<Settings>;
};
const enginePresetStorage = "soliton.engine-presets.v1";
const engineWords = {
  en: { preset: "Preset", current: "Current settings", name: "Preset name", save: "Save preset", details: "Advanced settings" },
  ja: { preset: "プリセット", current: "現在の設定", name: "プリセット名", save: "プリセットを保存", details: "詳細設定" },
  zh: { preset: "预设", current: "当前设置", name: "预设名称", save: "保存预设", details: "高级设置" },
};
function readEnginePresets(): EnginePreset[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(enginePresetStorage) ?? "[]");
    return Array.isArray(value) ? value.filter((item) =>
      item && Object.hasOwn(enginePresetKeys, item.engine) &&
      typeof item.name === "string" && item.settings && typeof item.settings === "object",
    ) : [];
  } catch {
    return [];
  }
}
function presetSettings(engine: Settings["engine"], source: Partial<Settings>) {
  return Object.fromEntries(enginePresetKeys[engine]
    .filter((key) => Object.hasOwn(source, key))
    .map((key) => [key, source[key]])) as Partial<Settings>;
}
type MediaKind = "image" | "audio" | "video";
type Attachment = { id: string; kind: MediaKind };
type PromptFile = { name: string; content: string };
const promptFileLimit = 64 * 1024;
const promptFileCount = 8;
const promptPdfLimit = 16 * 1024 * 1024;
const promptFileType = /\.(txt|md|markdown|csv|json|pdf)$/i;
const isPromptPdf = (file: File) => /\.pdf$/i.test(file.name);
const promptFileWords = {
  en: { add: "Attach files", remove: "Remove file", invalid: "Choose up to eight UTF-8 text files or selectable-text PDFs. Extracted text must total at most 64 KiB. Each PDF must fit within 16 MiB and 200 pages.", folder: "Choose folder", include: "Attach selected", close: "Close folder", empty: "No supported text files in this folder.", tooMany: "Choose a folder with at most 500 supported text files." },
  ja: { add: "ファイルを添付", remove: "ファイルを削除", invalid: "UTF-8のテキストファイルまたはテキスト選択可能なPDFを8件まで選べます。本文は合計64 KiBまで、PDFは1件16 MiB・200ページまでです。", folder: "フォルダを選択", include: "選んだファイルを添付", close: "フォルダ一覧を閉じる", empty: "対応するテキストファイルがありません。", tooMany: "対応ファイルが500件以下のフォルダを選択してください。" },
  zh: { add: "添加文件", remove: "移除文件", invalid: "请选择最多八个 UTF-8 文本文件或可选中文本的 PDF。正文总量不得超过 64 KiB，每个 PDF 不超过 16 MiB 和 200 页。", folder: "选择文件夹", include: "添加所选文件", close: "关闭文件夹列表", empty: "此文件夹没有受支持的文本文件。", tooMany: "请选择包含不超过 500 个受支持文本文件的文件夹。" },
};
type ChatBudget = { inputTokens: number | null; outputTokens: number; context: number; fits: boolean };
const chatBudgetWords = {
  en: { check: "Check token budget", pending: "Counting tokens…", input: "Input", output: "Reserved output", context: "Context", over: "Over limit" },
  ja: { check: "トークン数を確認", pending: "トークンを計数中…", input: "入力", output: "出力予約", context: "上限", over: "上限超過" },
  zh: { check: "检查令牌预算", pending: "正在计算令牌…", input: "输入", output: "预留输出", context: "上下文上限", over: "超出上限" },
};
type Message = { role: string; content: string; inference?: Inference; attachments?: Attachment[] };
const mediaKinds: Partial<Record<string, MediaKind>> = {
  png: "image", jpg: "image", jpeg: "image", wav: "audio", mp4: "video",
};
const imageInputWords = {
  en: { attach: "Attach image", invalid: "Choose a PNG or JPEG image." },
  ja: { attach: "画像を添付", invalid: "PNGまたはJPEG画像を選択してください。" },
  zh: { attach: "添加图片", invalid: "请选择 PNG 或 JPEG 图片。" },
};
const mediaWords = {
  en: { attach: "Attach media", remove: "Remove attachment", unsupported: "Choose a PNG, JPEG, WAV, or MP4 file.", image: "Image", audio: "Audio", video: "Video" },
  ja: { attach: "メディアを添付", remove: "添付を削除", unsupported: "PNG、JPEG、WAV、MP4のファイルを選択してください。", image: "画像", audio: "音声", video: "動画" },
  zh: { attach: "添加媒体附件", remove: "移除附件", unsupported: "请选择 PNG、JPEG、WAV 或 MP4 文件。", image: "图片", audio: "音频", video: "视频" },
};
type Snapshot = {
  inklingVision?: boolean;
  engineSession?: string;
  engineCapabilities?: Profile[];
  modelPlan?: ModelPlanState;
  vllmInspection?: VllmInspection | null;
  requests?: Metrics[];
  dockingDiagnostics?: { enabled: boolean };
  status: "offline" | "starting" | "ready";
  settings: Settings;
  document: {
    workspaceMode?: "inference" | "knowledge";
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
  strataModel?: string | null;
};
const defaults: Settings = {
  omni: { runtimeDir: "", model: "", projector: "", mediaRoot: "", evidence: "", realtimeEvidence: "", context: 4096 },

  prefixCache: defaultPrefixSettings,
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
const windowParams = new URLSearchParams(location.search);
const panelView = windowParams.get("panel");
const preferencesWindow = panelView === "preferences";
const initialTab = windowParams.get("tab");
const initialPreferenceTab: PreferenceTab =
  initialTab === "engine" || initialTab === "api" || initialTab === "status" ? initialTab : "general";
function App() {
  const [conversationsOpen, setConversationsOpen] = useState(
    () => window.matchMedia("(min-width: 820px)").matches,
  );
  const historyToggle = useRef<HTMLButtonElement>(null);
  const [modelsOpen, setModelsOpen] = useState(false);
  const [setupLocalView, setSetupLocalView] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [enginePresets, setEnginePresets] = useState(readEnginePresets);
  const [presetName, setPresetName] = useState("");
  const [preferenceTab, setPreferenceTab] = useState<PreferenceTab>(initialPreferenceTab);
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
  const readableError = error.includes("Prompt plus requested output exceeds the qualified context")
    ? {
        ja: "入力と出力上限の合計が検証済みコンテキストを超えています。環境設定 → APIの出力トークン上限を下げるか、会話を短くしてください。",
        en: "The prompt plus output limit exceeds the qualified context. Lower the output token limit in Preferences → API, or shorten the conversation.",
        zh: "输入和输出上限之和超过了已验证的上下文限制。请在偏好设置 → API中降低输出令牌上限，或缩短对话。",
      }[language]
    : error;

  const [online, setOnline] = useState(false);
  const [connectionFailed, setConnectionFailed] = useState(false);
  const [pending, setPending] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [prompt, setPrompt] = useState("");
  const [budgetPreview, setBudgetPreview] = useState<{ key: string; budget: ChatBudget } | null>(null);
  const [checkingBudget, setCheckingBudget] = useState(false);
  const [promptFiles, setPromptFiles] = useState<PromptFile[]>([]);
  const [readingPromptFiles, setReadingPromptFiles] = useState(false);
  const [folderFiles, setFolderFiles] = useState<File[]>([]);
  const [folderSelection, setFolderSelection] = useState<Set<string>>(new Set());
  const promptFolderPicker = useRef<HTMLInputElement>(null);
  const promptFilePicker = useRef<HTMLInputElement>(null);
  const [mediaDraft, setMediaDraft] = useState<{ path: string; name: string; kind: MediaKind } | null>(null);
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
  const workspaceMode = snapshot?.document.workspaceMode || "inference";
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
  const chatVisibleHere = workspaceMode === "inference"
    && (!panelView || workspaceWindow)
    && groups.some((group) =>
      group.active === "chat"
      && (windowGroup ? hosts[group.id] === windowGroup : !detached.includes(group.id)),
    );
  const structureDisabled =
    !online ||
    pending ||
    (!desktop &&
      detached.some((id) => id === "workspace" || id.startsWith("group-")));
  function setWorkspaceTab(tab: WorkspaceTab) {
    void run(async () => {
      if (workspaceMode !== "inference")
        await action("workspaceMode", { mode: "inference" });
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
  useEffect(() => {
    if (!desktop) return;
    const unlisten = listen("workspace-show-conversations", () => setConversationsOpen(true));
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
  const setupGuided = useRef(false);
  useEffect(() => {
    if (!desktop || panelView || !online || setupGuided.current) return;
    setupGuided.current = true;
    if (settings.engine === "strata" && !settings.config && !settings.mock) {
      void invoke("installation_status").then((failure) => {
        if (!failure) return inspectSetup().then(() => setSetupOpen(true));
      }).catch((e) => setError(String(e)));
    }
  }, [online]);
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
  async function selectMedia() {
    const epoch = conversationEpoch.current;
    try {
      const path = await invoke<string | null>("select_path", { kind: "omni-media" });
      if (!path || epoch !== conversationEpoch.current || busyRef.current) return;
      const name = path.split(/[\\/]/).at(-1) || path;
      const kind = mediaKinds[name.split(".").at(-1)?.toLowerCase() || ""];
      if (!kind || (snapshot?.inklingVision && kind !== "image")) {
        setError(snapshot?.inklingVision ? imageInputWords[language].invalid : mediaWords[language].unsupported);
        return;
      }
      setBudgetPreview(null);
      setMediaDraft({ path, name, kind });
    } catch (e) {
      setError(String(e));
    }
  }

  function closePromptFolder() {
    setFolderFiles([]);
    setFolderSelection(new Set());
  }

  function selectPromptFolder(files: FileList | null) {
    if (!files?.length) return;
    closePromptFolder();
    const candidates = Array.from(files).filter(file => promptFileType.test(file.name));
    if (!candidates.length || candidates.length > 500) {
      setError(candidates.length ? promptFileWords[language].tooMany : promptFileWords[language].empty);
      return;
    }
    setError("");
    setFolderFiles(candidates.sort((a, b) => a.webkitRelativePath.localeCompare(b.webkitRelativePath)));
  }

  async function selectPromptFiles(files: File[]) {
    if (!files.length) return;
    const epoch = conversationEpoch.current;
    const invalid = promptFileWords[language].invalid;
    const encoder = new TextEncoder();
    const existingBytes = promptFiles.reduce((total, file) => total + encoder.encode(file.content).length, 0);
    if (files.length + promptFiles.length > promptFileCount ||
        files.some(file => file.size > (isPromptPdf(file) ? promptPdfLimit : promptFileLimit)) ||
        files.some(file => !promptFileType.test(file.name))) {
      setError(invalid);
      return;
    }
    setReadingPromptFiles(true);
    try {
      const selected: PromptFile[] = [];
      let remaining = promptFileLimit - existingBytes;
      for (const file of files) {
        if (epoch !== conversationEpoch.current) return;
        const content = isPromptPdf(file)
          ? await (await import("./promptPdf")).extractPromptPdf(file, remaining)
          : new TextDecoder("utf-8", { fatal: true }).decode(await file.arrayBuffer());
        remaining -= encoder.encode(content).length;
        if (remaining < 0 || !content.trim() || content.includes("\0")) throw new Error(invalid);
        selected.push({ name: file.webkitRelativePath || file.name, content });
      }
      if (epoch === conversationEpoch.current && !busyRef.current) {
        setError("");
        setPromptFiles(current => [...current, ...selected]);
        return true;
      }
    } catch {
      if (epoch === conversationEpoch.current) setError(invalid);
    } finally {
      setReadingPromptFiles(false);
    }
  }

  const draftText = [prompt.trim(), ...promptFiles.map(file =>
      `--- ${JSON.stringify(file.name)} ---\n${file.content}`
    )].filter(Boolean).join("\n\n");
  const budgetConversation = snapshot?.document.conversations?.activeId;
  const canAttachMedia = snapshot?.settings.engine === "lfm3Omni" || snapshot?.inklingVision;
  const hasBudgetDraft = Boolean(draftText || mediaDraft);
  const canCheckTextBudget = snapshot && !mediaDraft &&
    !messages.some(message => message.attachments?.length) &&
    (snapshot.backend === "Inkling Metal (experimental)" || snapshot.settings.engine === "mlxPrefix" ||
      (snapshot.settings.engine === "vllmApple" && snapshot.settings.vllm.backend === "mlx_lm" &&
        Boolean(snapshot.settings.vllm.evidence)));
  const canCheckBudget = snapshot?.status === "ready" && (canAttachMedia || canCheckTextBudget);
  const budgetKey = canCheckBudget ? JSON.stringify([
    budgetConversation, snapshot.engineSession, settings.maxTokens, draftText, mediaDraft,
    messages.map(message => [message.role, message.content, message.attachments]),
  ]) : "";
  const shownBudget = budgetPreview?.key === budgetKey ? budgetPreview.budget : null;
  const budgetWords = chatBudgetWords[language];

  async function checkDraftBudget() {
    if (!canCheckBudget || !budgetConversation || !hasBudgetDraft || busyRef.current || checkingBudget) return;
    const epoch = conversationEpoch.current;
    setCheckingBudget(true);
    setBudgetPreview(null);
    setError("");
    try {
      const budget: ChatBudget = await request("/studio/chat/budget", {
        conversationId: budgetConversation, content: draftText, max_tokens: settings.maxTokens,
        ...(mediaDraft ? { media: { path: mediaDraft.path, kind: mediaDraft.kind } } : {}),
      }).then(response => response.json());
      if (epoch === conversationEpoch.current) setBudgetPreview({ key: budgetKey, budget });
    } catch (error) {
      if (epoch === conversationEpoch.current) setError((error as Error).message);
    } finally {
      setCheckingBudget(false);
    }
  }

  async function send() {
    if (
      (!prompt.trim() && !mediaDraft && !promptFiles.length) ||
      (mediaDraft && !canAttachMedia) ||
      busyRef.current ||
      readingPromptFiles ||
      busy ||
      snapshot?.document.generating ||
      snapshot?.status !== "ready"
    )
      return;
    setError("");
    followOutput.current = true;
    const text = draftText;
    busyRef.current = true;
    setBusy(true);
    const c = new AbortController();
    controller.current = c;
    let answer = "";
    try {
      await persist();
      let attachment: Attachment | undefined;
      if (mediaDraft) {
        const response = await request("/studio/omni/assets/import", {
          path: mediaDraft.path,
          kind: mediaDraft.kind,
        }, c.signal);
        attachment = await response.json();
      }
      c.signal.throwIfAborted();
      const message: Message = { role: "user", content: text };
      if (attachment) message.attachments = [attachment];
      const next = [...messages, message];
      setPrompt(current => current === prompt ? "" : current);
      setMediaDraft(null);
      setPromptFiles([]);
      closePromptFolder();
      setMessages([...next, { role: "assistant", content: "" }]);
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
  const runningSettings = status === "offline" ? undefined : snapshot?.settings;
  const engineLabel = (selected: Settings) => ({
    strata: selected.mock ? t.mock : "Strata",
    vllmApple: "vLLM-Apple",
    mlxPrefix: "MLX SSD",
    lfm3Omni: "LFM3-Omni",
  })[selected.engine];
  const configuredModel = (selected: Settings) => ({
    strata: !selected.mock && selected.config === snapshot?.settings.config
      ? snapshot.strataModel : undefined,
    vllmApple: selected.vllm.model,
    mlxPrefix: selected.prefixCache.model,
    lfm3Omni: selected.omni.model,
  })[selected.engine]?.trim().split(/[\\/]/).filter(Boolean).at(-1);
  const engineName = runningSettings && engineLabel(runningSettings);
  const reportedModel = snapshot?.health?.model?.trim();
  const modelName = status === "ready" && runningSettings &&
    !(runningSettings.engine === "strata" && runningSettings.mock)
    ? (reportedModel !== "default_model" && reportedModel) ||
      configuredModel(runningSettings)
    : undefined;
  const runtimeLabel = [snapshot?.architecture, engineName, modelName]
    .filter(Boolean)
    .join(" / ") || "—";

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
        draggable={desktop && !!panelView && !preferencesWindow}
        onDragStart={(e) => e.dataTransfer.setData("text/plain", id)}
      >
        <h2>
          {icon}
          {id === "engine" ? t.engineSettings : t[id]}
        </h2>
        <div className="tools">
          {extra}
          {panelView &&
            !preferencesWindow &&
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
  function engineStatus() {
    const selected = snapshot?.settings ?? settings;
    return (
      <section className="panel engine-status">
        <header className="panel-header">
          <h2><Activity />{t.status}</h2>
        </header>
        <div className="panel-body engine-status-body">
          <div className="value-row">
            <span>{t.status}</span>
            <span className={"status " + status}><i />{t[status]}</span>
          </div>
          <div className="value-row">
            <span>{t.platform}</span>
            <code>{snapshot?.architecture || "—"} / {snapshot?.backend || "—"}</code>
          </div>
          {preparing && <p className="test-label" role="status">{t.setupRunning}</p>}
          {runningSettings?.engine === "strata" && runningSettings.mock &&
            <p className="test-label">{t.testMode}</p>}
          <div className="engine-memory-overview">
            <MemoryUsage
              usage={snapshot?.memoryUsage}
              language={language}
              selectedEngine={engineLabel(selected)}
              selectedModel={modelName || configuredModel(selected)}
            />
            <MemoryHistory
              history={snapshot?.memoryUsage?.history}
              events={snapshot?.memoryUsage?.events}
              language={language}
            />
          </div>
          {snapshot?.modelPlan && (
            <ModelPlan
              value={snapshot.modelPlan}
              language={language}
              onRefresh={selected.engine === "vllmApple" ? async () => {
                await run(async () => {
                  try {
                    await action("refreshBackendBudget");
                  } catch {
                    throw new Error(t.planRuntimeError);
                  }
                });
              } : undefined}
              refreshDisabled={!online || pending || status === "offline"}
            />
          )}
        </div>
      </section>
    );
  }
  function engine() {
    const labels = engineWords[language];
    const presets = enginePresets.filter((item) => item.engine === settings.engine);
    const currentPreset = presets.find((item) =>
      JSON.stringify(presetSettings(item.engine, item.settings)) ===
      JSON.stringify(presetSettings(settings.engine, settings)),
    );
    const presetLocked = !online || pending || preparing || status !== "offline";
    const name = presetName.trim();
    const nameTaken = presets.some((item) => item.name === name);
    function savePreset() {
      void run(async () => {
        if (dirty.current) await persist();
        const engine = draftRef.current.engine;
        const next = [...enginePresets, { engine, name, settings: presetSettings(engine, draftRef.current) }];
        localStorage.setItem(enginePresetStorage, JSON.stringify(next));
        setEnginePresets(next);
        setPresetName("");
      });
    }
    function applyPreset(name: string) {
      const preset = presets.find((item) => item.name === name);
      if (!preset) return;
      void run(async () => {
        const next = { ...draftRef.current, ...presetSettings(preset.engine, preset.settings) };
        await action("settings", { settings: next });
        draftRef.current = next;
        setSettings(next);
        dirty.current = false;
      });
    }

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
        <div className="panel-body engine-settings-body">
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
              <option value="mlxPrefix">MLX SSD</option>
              <option value="lfm3Omni">LFM3-Omni</option>
            </select>
          </label>
          <label>
            {labels.preset}
            <select
              aria-label={labels.preset}
              value={currentPreset?.name ?? ""}
              disabled={presetLocked || presets.length === 0}
              onChange={(event) => applyPreset(event.target.value)}
            >
              <option value="" disabled>{labels.current}</option>
              {presets.map((preset) => <option key={preset.name}>{preset.name}</option>)}
            </select>
          </label>
          {snapshot?.engineCapabilities && <EngineGuide profiles={snapshot.engineCapabilities} language={language} />}
          <details className="engine-details" key={settings.engine}>
            <summary>{labels.details}</summary>
            <div className="engine-details-content">
              <div className="engine-preset-save">
                <input
                  aria-label={labels.name}
                  placeholder={labels.name}
                  value={presetName}
                  maxLength={60}
                  onChange={(event) => setPresetName(event.target.value)}
                />
                <button type="button" disabled={presetLocked || !name || nameTaken} onClick={savePreset}>
                  {labels.save}
                </button>
              </div>
              {settings.engine === "mlxPrefix" ? (
                <PrefixEngineSettings value={settings.prefixCache || defaultPrefixSettings} language={language}
                  locked={status !== "offline" || pending} running={status === "ready"}
                  onChange={(value) => update("prefixCache", value)} onSave={persist} />
              ) : settings.engine === "lfm3Omni" ? (
                <OmniSettings language={language} value={settings.omni}
                  locked={status !== "offline" || pending}
                  onChange={value => update("omni", value)}
                  onSave={() => void persist().catch((e: Error) => setError(e.message))} />
              ) : settings.engine === "strata" ? (
                <>
                  {snapshot?.backend === "Metal" && (
                    <MemoryControls
                      value={settings.metalMemory ?? defaultMemory}
                      tuning={snapshot?.memoryTuning}
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
                  <label>
                    {t.mpsFallback}
                    <span title={t.mpsFallbackHelp}><Info size={13} /></span>
                    <select
                      aria-label={t.mpsFallback}
                      value={settings.vllm.mpsFallback == null ? "inherit" : settings.vllm.mpsFallback ? "1" : "0"}
                      disabled={status !== "offline" || pending}
                      onChange={(e) => {
                        update("vllm", {
                          ...settings.vllm,
                          mpsFallback: e.target.value === "inherit" ? null : e.target.value === "1",
                        });
                        void run(persist);
                      }}
                    >
                      <option value="inherit">{t.mpsInherit}</option>
                      <option value="1">{t.mpsEnabled}</option>
                      <option value="0">{t.mpsDisabled}</option>
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
                </>
              )}
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
            </div>
          </details>
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
        <div className="chat-layout">
        <ConversationLibrary
          library={snapshot?.document.conversations}
          language={language}
          open={conversationsOpen}
          disabled={busy || !!snapshot?.document.generating || pending || !online}
          onAction={conversationAction}
          onClose={() => {
            setConversationsOpen(false);
            historyToggle.current?.focus();
          }}
        />
        <div className="chat-main">
        <header className="panel-header">
          <h2>
            <MessageSquare />
            {t.chat}
          </h2>
          {!conversationsOpen && button(
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
                <div className="message-author">{m.role === "user" ? t.you : "Soliton"}</div>
                {m.role === "assistant" && (
                  <InferenceView value={m.inference} language={language} />
                )}
                {m.attachments?.map(asset => (
                  <span className="media-chip" key={asset.id}>{mediaWords[language][asset.kind]}</span>
                ))}
                {m.content ? (
                  <ChatMath content={m.content} language={language} />
                ) : m.attachments?.length ? null : !m.inference || m.inference.metrics.status === "running" ? (
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
        {status === "ready" && snapshot?.settings.engine === "lfm3Omni" && (
          <OmniVoice key={language} language={language} />
        )}
        {mediaDraft && (
          <div className="media-draft">
            <span className="media-chip" title={mediaDraft.name}>{mediaDraft.name}</span>
            <button type="button" className="icon-button" disabled={busy}
              aria-label={mediaWords[language].remove} onClick={() => setMediaDraft(null)}><X size={14} /></button>
          </div>
        )}
        {promptFiles.length > 0 && (
          <div className="media-draft prompt-files">
            {promptFiles.map((file, index) => (
              <span className="prompt-file" key={index}>
                <span className="media-chip" title={file.name}>{file.name}</span>
                <button type="button" className="icon-button" disabled={busy}
                  aria-label={`${promptFileWords[language].remove} · ${file.name}`}
                  onClick={() => setPromptFiles(current => current.filter((_, position) => position !== index))}>
                  <X size={14} />
                </button>
              </span>
            ))}
          </div>
        )}
        {folderFiles.length > 0 && (
          <section className="prompt-folder" aria-label={promptFileWords[language].folder}>
            <div className="prompt-folder-list">
              {folderFiles.map(file => {
                const path = file.webkitRelativePath;
                return <label key={path}>
                  <input type="checkbox" checked={folderSelection.has(path)}
                    disabled={busy || readingPromptFiles}
                    onChange={event => {
                      const checked = event.currentTarget.checked;
                      setFolderSelection(current => {
                        const next = new Set(current);
                        if (checked) next.add(path);
                        else next.delete(path);
                        return next;
                      });
                    }} />
                  <span title={path}>{path}</span>
                </label>;
              })}
            </div>
            <div className="prompt-folder-actions">
              <button type="button" disabled={busy || readingPromptFiles || !folderSelection.size}
                onClick={async () => {
                  if (await selectPromptFiles(folderFiles.filter(file => folderSelection.has(file.webkitRelativePath)))) {
                    closePromptFolder();
                  }
                }}>{promptFileWords[language].include} ({folderSelection.size})</button>
              <button type="button" className="icon-button" disabled={readingPromptFiles}
                aria-label={promptFileWords[language].close} onClick={closePromptFolder}><X size={14} /></button>
            </div>
          </section>
        )}
        {canCheckBudget && hasBudgetDraft && (
          <div className="chat-budget">
            <button type="button"
              disabled={busy || checkingBudget || readingPromptFiles || snapshot?.document.generating}
              onClick={() => void checkDraftBudget()}>{checkingBudget ? budgetWords.pending : budgetWords.check}</button>
            {shownBudget && <output>
              {shownBudget.inputTokens !== null && <>{budgetWords.input} {shownBudget.inputTokens.toLocaleString()} · </>}{budgetWords.output} {shownBudget.outputTokens.toLocaleString()} · {budgetWords.context} {shownBudget.context.toLocaleString()}
              {!shownBudget.fits && <strong> · {budgetWords.over}</strong>}
            </output>}
          </div>
        )}
        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault();
            send();
          }}
        >
          <input ref={promptFilePicker} type="file" hidden multiple
            accept=".txt,.md,.markdown,.csv,.json,.pdf"
            onChange={event => {
              const files = Array.from(event.currentTarget.files ?? []);
              event.currentTarget.value = "";
              void selectPromptFiles(files);
            }} />
          <button type="button" disabled={busy || readingPromptFiles || snapshot?.document.generating}
            aria-label={promptFileWords[language].add} title={promptFileWords[language].add}
            onClick={() => promptFilePicker.current?.click()}>
            <FileText size={16} />
          </button>
          <input ref={promptFolderPicker} type="file" hidden {...{ webkitdirectory: "" }}
            onChange={event => {
              selectPromptFolder(event.currentTarget.files);
              event.currentTarget.value = "";
            }} />
          <button type="button" disabled={busy || readingPromptFiles || snapshot?.document.generating}
            aria-label={promptFileWords[language].folder} title={promptFileWords[language].folder}
            onClick={() => promptFolderPicker.current?.click()}>
            <FolderOpen size={16} />
          </button>
          {desktop && status === "ready" && canAttachMedia && (
            <button type="button" disabled={busy || snapshot?.document.generating}
              aria-label={snapshot?.inklingVision ? imageInputWords[language].attach : mediaWords[language].attach}
              title={snapshot?.inklingVision ? imageInputWords[language].attach : mediaWords[language].attach}
              onClick={() => void selectMedia()}><Paperclip size={16} /></button>
          )}
          <textarea
            aria-label={t.placeholder}
            placeholder={t.placeholder}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => {
              if (
                e.key === "Enter" &&
                e.shiftKey &&
                !e.nativeEvent.isComposing &&
                e.nativeEvent.keyCode !== 229
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
              aria-keyshortcuts="Shift+Enter"
              title={t.send}
              disabled={
                status !== "ready" ||
                readingPromptFiles ||
                snapshot?.document.generating ||
                (mediaDraft && !canAttachMedia) ||
                (!prompt.trim() && !mediaDraft && !promptFiles.length)
              }
            >
              <Send />
            </button>
          )}
        </form>
        </div>
        </div>
      </section>
    );
  }
  function openPreferences(tab: PreferenceTab = "general") {
    if (desktop) {
      if (!preferencesWindow) {
        void invoke("open_preferences", { tab, title: t.preferences.replace("…", "") })
          .catch((e: unknown) => setError(String(e)));
      }
      return;
    }
    setPreferenceTab(tab);
    setPreferencesOpen(true);
  }
  async function closePreferences() {
    try {
      if (dirty.current && !preparing) await persist();
      if (preferencesWindow) await invoke("close_preferences");
      else setPreferencesOpen(false);
    } catch (e) {
      setError(String((e as Error).message));
    }
  }
  const closePreferencesWindow = useEffectEvent(closePreferences);
  useEffect(() => {
    if (!preferencesWindow || !desktop) return;
    const unlisten = getCurrentWindow().onCloseRequested((event) => {
      event.preventDefault();
      void closePreferencesWindow();
    });
    return () => { void unlisten.then(dispose => dispose()); };
  }, []);
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
      ) {
        setPrompt("");
        setMediaDraft(null);
        setPromptFiles([]);
        closePromptFolder();
      }
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
          action: () => {
            setWorkspaceTab("chat");
            setConversationsOpen(true);
            if (desktop)
              void emit("workspace-show-conversations").catch((error: unknown) =>
                setError(String(error)),
              );
          },
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
    <div className={"workbench app-shell " + (panelView && !preferencesWindow ? "floating" : "")}>
      {!preferencesWindow && (<>
        <header className="app-header">
          <AppMenus menus={menus} onBeforeOpen={captureEditor} />
          <div className="header-actions">
            {chatVisibleHere && (
              <button
                type="button"
                ref={historyToggle}
                className="icon-button"
                title={conversationsOpen ? t.hideConversationHistory : t.showConversationHistory}
                aria-label={conversationsOpen ? t.hideConversationHistory : t.showConversationHistory}
                aria-expanded={conversationsOpen}
                aria-controls="conversation-sidebar"
                onClick={() => setConversationsOpen((open) => !open)}
              >
                {conversationsOpen ? <PanelLeftClose /> : <PanelLeftOpen />}
              </button>
            )}
            {button(<SettingsIcon />, t.preferences, () => openPreferences())}
          </div>
        </header>
        {!online ? (
          <div className="notice" role="status">
            {connectionFailed ? t.connectionError : t.connecting}
          </div>
        ) : null}
        {error ? (
          <div className="notice error" role="alert">
            {readableError}
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
          <main className="workspace-host workspace-with-modes">
            <div
              className="workspace-mode-tabs"
              role="tablist"
              aria-label="Soliton"
            >
              {(["inference", "knowledge"] as const).map((mode) => (
                <button
                  key={mode}
                  id={`mode-${mode}`}
                  role="tab"
                  aria-selected={workspaceMode === mode}
                  aria-controls={`mode-content-${mode}`}
                  tabIndex={workspaceMode === mode ? 0 : -1}
                  disabled={!online || pending}
                  onClick={() =>
                    void run(() => action("workspaceMode", { mode }))
                  }
                  onKeyDown={(e) => {
                    if (
                      !["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)
                    )
                      return;
                    e.preventDefault();
                    const next =
                      e.key === "Home"
                        ? "inference"
                        : e.key === "End"
                          ? "knowledge"
                          : mode === "inference"
                            ? "knowledge"
                            : "inference";
                    void run(() => action("workspaceMode", { mode: next })).then(
                      () => document.getElementById(`mode-${next}`)?.focus(),
                    );
                  }}
                >
                  {mode === "inference" ? (
                    <MessageSquare size={15} />
                  ) : (
                    <Layers size={15} />
                  )}
                  {knowledgeWords[language][mode]}
                </button>
              ))}
              {workspaceMode === "knowledge" && desktop && (
                <button
                  className="icon-button knowledge-float"
                  title={windowGroup ? t.dockGroup : t.floatGroup}
                  aria-label={windowGroup ? t.dockGroup : t.floatGroup}
                  disabled={pending}
                  onClick={() =>
                    windowGroup
                      ? dockWorkspace(currentGroup)
                      : float(currentGroup)
                  }
                >
                  {windowGroup ? (
                    <PanelLeftClose size={15} />
                  ) : (
                    <ExternalLink size={15} />
                  )}
                </button>
              )}
            </div>
            <div
              id="mode-content-inference"
              role="tabpanel"
              aria-labelledby="mode-inference"
              hidden={workspaceMode !== "inference"}
              className="workspace-mode-content"
            >
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
            </div>
            <div
              id="mode-content-knowledge"
              role="tabpanel"
              aria-labelledby="mode-knowledge"
              hidden={workspaceMode !== "knowledge"}
              className="workspace-mode-content"
            >
              <KnowledgeLab
                language={language}
                active={workspaceMode === "knowledge"}
                online={online}
                engineOffline={snapshot?.status === "offline"}
              />
            </div>
          </main>
        )}
        <InstallationRepair language={language} />
      </>)}
      {(preferencesOpen || preferencesWindow) && (
        <PreferencesDialog
          language={language}
          tab={preferenceTab}
          onTab={setPreferenceTab}
          onClose={closePreferences}
          error={readableError}
          standalone={preferencesWindow}
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
          ) : preferenceTab === "status" ? (
            engineStatus()
          ) : preferenceTab === "engine" ? (
            engine()
          ) : (
            api()
          )}
        </PreferencesDialog>
      )}
      {modelsOpen && (
        <ModelLibraryDialog
          profiles={snapshot?.engineCapabilities ?? []}
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
        <span className="statusbar-runtime" title={runtimeLabel}>
          <Monitor size={14} aria-hidden="true" />
          <span>{runtimeLabel}</span>
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
