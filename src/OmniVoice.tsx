import { useEffect, useRef, useState } from "react";
import { Mic, Square, Volume2, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { desktop, request, realtimeSocket } from "./client";
import type { Language } from "./i18n";

const labels = {
  en: { record: "Record voice", send: "Stop and send", cancel: "Cancel voice turn", end: "End voice session", play: "Play response", failed: "Voice session failed", audioFailed: "Could not load the voice response", connecting: "Connecting voice session…", waiting: "Waiting for response…", generating: "Generating voice response…", loadingAudio: "Preparing voice playback…", cancelPlayback: "Cancel voice playback", cancelling: "Cancelling voice response…", cancelled: "Voice response cancelled" },
  ja: { record: "音声を録音", send: "録音を停止して送信", cancel: "音声ターンをキャンセル", end: "音声対話を終了", play: "応答を再生", failed: "音声対話に失敗しました", audioFailed: "応答音声を読み込めませんでした", connecting: "音声対話に接続中…", waiting: "応答を待っています…", generating: "音声応答を生成中…", loadingAudio: "音声を再生準備中…", cancelPlayback: "音声再生をキャンセル", cancelling: "音声応答をキャンセル中…", cancelled: "音声応答をキャンセルしました" },
  zh: { record: "录制语音", send: "停止录音并发送", cancel: "取消语音轮次", end: "结束语音对话", play: "播放回复", failed: "语音对话失败", audioFailed: "无法加载回复音频", connecting: "正在连接语音对话…", waiting: "正在等待回复…", generating: "正在生成语音回复…", loadingAudio: "正在准备播放语音…", cancelPlayback: "取消语音播放", cancelling: "正在取消语音回复…", cancelled: "语音回复已取消" },
};

type Recording = {
  stream: MediaStream;
  context: AudioContext;
  processor: ScriptProcessorNode;
  source: MediaStreamAudioSourceNode;
  chunks: Float32Array[];
  length: number;
};

function release(recording: Recording) {
  recording.processor.onaudioprocess = null;
  recording.source.disconnect();
  recording.processor.disconnect();
  recording.stream.getTracks().forEach(track => track.stop());
  void recording.context.close();
}

const nativeMicrophone = desktop && /Macintosh/.test(navigator.userAgent);
const RATE = 16000;
const MAX_SECONDS = 30;
type Phase = "idle" | "connecting" | "recording" | "waiting";

export function OmniVoice({ language }: { language: Language }) {
  const t = labels[language];
  const [phase, setPhase] = useState<Phase>("idle");
  const [responseStatus, setResponseStatus] = useState<"waiting" | "generating" | "loadingAudio" | "cancelling" | "cancelled">("waiting");
  const [error, setError] = useState<false | true | "audio">(false);
  const [audio, setAudio] = useState("");
  const [transcript, setTranscript] = useState("");
  const socket = useRef<WebSocket | null>(null);
  const recording = useRef<Recording | null>(null);
  const nativeRecording = useRef<number | null>(null);
  const playback = useRef("");
  const audioRequest = useRef<AbortController | null>(null);
  const turn = useRef<"unsent" | "submitted" | "admitted">("unsent");
  const cancelled = useRef(false);
  const generation = useRef(0);

  function clearAudio() {
    audioRequest.current?.abort();
    audioRequest.current = null;
    if (playback.current) URL.revokeObjectURL(playback.current);
    playback.current = "";
    setAudio("");
  }

  function stopNative(keep: boolean) {
    if (nativeRecording.current === null) return;
    window.clearTimeout(nativeRecording.current);
    nativeRecording.current = null;
    return invoke<string[]>("microphone_stop", { keep });
  }

  function end() {
    void stopNative(false)?.catch(() => {});
    generation.current++;
    const active = recording.current;
    recording.current = null;
    if (active) release(active);
    const ws = socket.current;
    socket.current = null;
    ws?.close();
    turn.current = "unsent";
    setResponseStatus("waiting");
    clearAudio();
    setPhase("idle");
  }

  useEffect(() => () => {
    generation.current++;
    void stopNative(false)?.catch(() => {});
    socket.current?.close();
    audioRequest.current?.abort();
    if (recording.current) release(recording.current);
    if (playback.current) URL.revokeObjectURL(playback.current);
  }, []);

  const send = (ws: WebSocket, event: object) => ws.send(JSON.stringify(event));

  async function connect() {
    if (socket.current?.readyState === WebSocket.OPEN) return socket.current;
    const ws = realtimeSocket();
    socket.current = ws;
    let ready: () => void = () => {};
    let failed: (error: Error) => void = () => {};
    const connected = new Promise<void>((resolve, reject) => { ready = resolve; failed = reject; });
    const timer = window.setTimeout(() => failed(new Error("Realtime connection timeout")), 120000);
    ws.onclose = () => {
      failed(new Error("Realtime disconnected"));
      if (socket.current === ws) { end(); setError(true); }
    };
    ws.onerror = () => failed(new Error("Realtime connection failed"));
    ws.onmessage = async ({ data }) => {
      if (socket.current !== ws) return;
      try {
        const event = JSON.parse(data);
        if (event.type === "session.created") {
          send(ws, { type: "session.update", session: {
            type: "realtime", max_output_tokens: 256, output_modalities: ["text", "audio"],
            instructions: {en:"Respond in interleaved text and audio.",ja:"Respond in interleaved text and audio in Japanese.",zh:"Respond in interleaved text and audio in Simplified Chinese."}[language],
          }});
        } else if (event.type === "session.updated") {
          ready();
        } else if (event.type === "soliton.realtime.admission") {
          if (event.accepted === true) {
            turn.current = "admitted";
            if (cancelled.current) send(ws, { type: "response.cancel" });
          }
        } else if (event.type === "response.output_text.delta" || event.type === "soliton.audio.buffered") {
          if (!cancelled.current) setResponseStatus("generating");
        } else if (event.type === "response.done") {
          turn.current = "unsent";
          const output = event.soliton_output;
          if (event.response?.status === "cancelled") setResponseStatus("cancelled");
          if (event.response?.status === "completed" && !cancelled.current) {
            setTranscript(output?.text || "");
            if (typeof output?.audioUrl === "string" && /^\/v1\/realtime\/audio\/[a-f0-9-]+$/.test(output.audioUrl)) {
              const controller = new AbortController();
              audioRequest.current = controller;
              setResponseStatus("loadingAudio");
              try {
                const result = await request(output.audioUrl, undefined, controller.signal);
                const blob = await result.blob();
                if (socket.current !== ws || controller.signal.aborted) return;
                playback.current = URL.createObjectURL(blob);
                setAudio(playback.current);
              } catch {
                if (socket.current !== ws || controller.signal.aborted) return;
                setError("audio");
              } finally {
                if (audioRequest.current === controller) audioRequest.current = null;
              }
            }
          }
          setPhase("idle");
        } else if (event.type === "error") {
          failed(new Error("Realtime request failed"));
          end();
          setError(true);
        }
      } catch (error) {
        failed(error instanceof Error ? error : new Error("Realtime response failed"));
        if (socket.current === ws) { end(); setError(true); }
      }
    };
    try {
      await connected;
      return ws;
    } finally {
      window.clearTimeout(timer);
    }
  }

  async function startRecording() {
    const current = ++generation.current;
    setError(false);
    setResponseStatus("waiting");
    setTranscript("");
    clearAudio();
    setPhase("connecting");
    let microphone: MediaStream | undefined;
    let context: AudioContext | undefined;
    try {
      if (nativeMicrophone) {
        let permission = 0;
        while (permission === 0 && generation.current === current) {
          permission = await invoke<number>("microphone_permission");
          if (permission === 0) await new Promise(resolve => setTimeout(resolve, 300));
        }
        if (generation.current !== current) return;
        if (permission !== 1) throw new Error("Microphone access denied");
        await connect();
        if (generation.current !== current) return;
        await invoke("microphone_start");
        if (generation.current !== current) {
          await invoke("microphone_stop", { keep: false });
          return;
        }
        nativeRecording.current = window.setTimeout(() => void finishRecording(), MAX_SECONDS * 1000);
        setPhase("recording");
        return;
      }
      context = new AudioContext();
      const resume = context.resume();
      microphone = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1 } });
      if (generation.current !== current) { microphone.getTracks().forEach(track => track.stop()); void context.close(); return; }
      await connect();
      if (generation.current !== current) { microphone.getTracks().forEach(track => track.stop()); void context.close(); return; }
      await resume;
      const source = context.createMediaStreamSource(microphone);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const active: Recording = { stream: microphone, context, processor, source, chunks: [], length: 0 };
      processor.onaudioprocess = (event) => {
        const samples = event.inputBuffer.getChannelData(0);
        const count = Math.min(samples.length, MAX_SECONDS * active.context.sampleRate - active.length);
        if (count > 0) {
          active.chunks.push(samples.slice(0, count));
          active.length += count;
        }
        if (active.length >= MAX_SECONDS * active.context.sampleRate) void finishRecording();
      };
      source.connect(processor);
      processor.connect(context.destination);
      recording.current = active;
      setPhase("recording");
    } catch {
      microphone?.getTracks().forEach(track => track.stop());
      if (context) void context.close();
      if (generation.current === current) { end(); setError(true); }
    }
  }

  async function finishRecording() {
    const current = generation.current;
    const active = recording.current;
    const ws = socket.current;
    if (!ws || (!active && nativeRecording.current === null)) return;
    if (!active) {
      cancelled.current = false;
      setPhase("waiting");
      try {
        const chunks = await stopNative(true);
        if (socket.current !== ws || generation.current !== current || cancelled.current || !chunks) return;
        for (const audio of chunks) send(ws, { type: "input_audio_buffer.append", audio });
        send(ws, { type: "input_audio_buffer.commit" });
        turn.current = "submitted";
        send(ws, { type: "response.create" });
      } catch {
        if (generation.current === current) { end(); setError(true); }
      }
      return;
    }
    recording.current = null;
    release(active);
    if (!active.length) { setPhase("idle"); return; }
    cancelled.current = false;
    setPhase("waiting");
    try {
      const rendered = new OfflineAudioContext(1, Math.max(1, Math.floor(active.length * RATE / active.context.sampleRate)), RATE);
      const input = rendered.createBuffer(1, active.length, active.context.sampleRate);
      const channel = input.getChannelData(0);
      let offset = 0;
      for (const chunk of active.chunks) { channel.set(chunk, offset); offset += chunk.length; }
      const source = rendered.createBufferSource();
      source.buffer = input;
      source.connect(rendered.destination);
      source.start();
      const samples = (await rendered.startRendering()).getChannelData(0);
      if (socket.current !== ws || generation.current !== current || cancelled.current) return;
      for (let start = 0; start < samples.length; start += RATE) {
        const pcm = new DataView(new ArrayBuffer(Math.min(RATE, samples.length - start) * 2));
        for (let i = 0; i < pcm.byteLength / 2; i++) {
          const value = Math.max(-1, Math.min(1, samples[start + i]));
          pcm.setInt16(i * 2, value * (value < 0 ? 32768 : 32767), true);
        }
        send(ws, { type: "input_audio_buffer.append", audio: btoa(String.fromCharCode(...new Uint8Array(pcm.buffer))) });
      }
      send(ws, { type: "input_audio_buffer.commit" });
      turn.current = "submitted";
      send(ws, { type: "response.create" });
    } catch {
      if (generation.current === current) { end(); setError(true); }
    }
  }

  function cancelTurn() {
    if (phase === "waiting" && cancelled.current) return;
    cancelled.current = true;
    const ws = socket.current;
    const loadingAudio = audioRequest.current !== null;
    clearAudio();
    if (loadingAudio) {
      setResponseStatus("waiting");
      setPhase("idle");
      return;
    }
    if (ws && phase === "waiting") {
      if (turn.current === "unsent") {
        generation.current++;
        setResponseStatus("waiting");
        setPhase("idle");
        return;
      }
      setResponseStatus("cancelling");
      if (turn.current === "admitted") send(ws, { type: "response.cancel" });
    } else if (phase === "recording") {
      if (recording.current) release(recording.current);
      void stopNative(false)?.catch(() => {});
      recording.current = null;
      setPhase("idle");
    } else {
      end();
    }
  }

  const cancelLabel = responseStatus === "loadingAudio" ? t.cancelPlayback : t.cancel;

  return (
    <div className="omni-voice">
      {phase === "idle" && <button type="button" onClick={() => void startRecording()} aria-label={t.record} title={t.record}><Mic size={16} /></button>}
      {phase === "recording" && <button type="button" onClick={() => void finishRecording()} aria-label={t.send} title={t.send}><Square size={16} /></button>}
      {phase !== "idle" && <button type="button" onClick={cancelTurn} aria-label={cancelLabel} title={cancelLabel}><X size={16} /></button>}
      {phase === "idle" && socket.current && <button type="button" onClick={end} aria-label={t.end} title={t.end}><X size={16} /></button>}
      {phase === "connecting" && <span role="status">{t.connecting}</span>}
      {(phase === "waiting" || responseStatus === "cancelled") && <span role="status">{t[responseStatus]}</span>}
      {error && <span role="alert">{error === "audio" ? t.audioFailed : t.failed}</span>}
      {transcript && <span>{transcript}</span>}
      {audio && <><Volume2 size={16} aria-hidden="true" /><audio controls autoPlay src={audio} aria-label={t.play} /></>}
    </div>
  );
}

const fieldLabels = {
  en: ["Native runtime folder", "GGUF model", "GGUF projector", "Private media folder", "Media qualification", "Realtime qualification"],
  ja: ["ネイティブランタイムのフォルダ", "GGUFモデル", "GGUFプロジェクター", "非公開メディアフォルダ", "メディア資格情報", "Realtime資格情報"],
  zh: ["原生运行时目录", "GGUF 模型", "GGUF 投影器", "私有媒体目录", "媒体资格凭据", "实时资格凭据"],
};
const fields = ["runtimeDir", "model", "projector", "mediaRoot", "evidence", "realtimeEvidence"] as const;
type OmniConfig = Record<(typeof fields)[number], string> & { context: number };

export function OmniSettings({ language, value, locked, onChange, onSave }: {
  language: Language;
  value: OmniConfig;
  locked: boolean;
  onChange: (value: OmniConfig) => void;
  onSave: () => void;
}) {
  return <div className="prefix-settings">
    {fields.map((field, index) => <label key={field}>
      {fieldLabels[language][index]}
      <input value={value[field] || ""} disabled={locked} spellCheck={false}
        onChange={event => onChange({ ...value, context: 4096, [field]: event.target.value })}
        onBlur={onSave} />
    </label>)}
  </div>;
}
