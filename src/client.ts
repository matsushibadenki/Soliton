import { invoke } from "@tauri-apps/api/core";
export const desktop = "__TAURI_INTERNALS__" in window;
let credentials: { url: string; token: string };
async function checked(r: Response) {
  if (!r.ok) {
    const b = await r.json().catch(() => null);
    throw new Error(b?.error?.message || "HTTP " + r.status);
  }
  return r;
}
export async function connect() {
  credentials = desktop
    ? await invoke("connection")
    : await fetch("/studio/session", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: "{}",
      })
        .then(checked)
        .then((r) => r.json());
  return credentials;
}
export function token() {
  return credentials?.token || "";
}
export async function request(
  path: string,
  body?: unknown,
  signal?: AbortSignal,
) {
  return checked(
    await fetch((desktop ? credentials.url : "") + path, {
      method: body === undefined ? "GET" : "POST",
      headers: {
        "Content-Type": "application/json",
        "x-studio-token": credentials.token,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    }),
  );
}
export async function action(
  action: string,
  data: Record<string, unknown> = {},
) {
  return request("/studio/action", { action, ...data }).then((r) => r.json());
}
export async function streamChat(
  body: unknown,
  signal: AbortSignal,
  onDelta: (text: string) => void,
) {
  const r = await request("/studio/chat", body, signal);
  if (!r.body) throw new Error("Empty response");
  const reader = r.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let done = false;
  function parse(event: string) {
    for (const line of event.split("\n")) {
      if (!line.startsWith("data:")) continue;
      const data = line.slice(5).trim();
      if (data === "[DONE]") {
        done = true;
        continue;
      }
      if (!data) continue;
      const v = JSON.parse(data);
      if (v.error) throw new Error(v.error.message);
      const delta = v.choices?.[0]?.delta?.content;
      if (delta) onDelta(delta);
    }
  }
  try {
    while (!done) {
      const chunk = await reader.read();
      buffer = (
        buffer + decoder.decode(chunk.value, { stream: !chunk.done })
      ).replace(/\r\n/g, "\n");
      let end;
      while ((end = buffer.indexOf("\n\n")) !== -1) {
        parse(buffer.slice(0, end));
        buffer = buffer.slice(end + 2);
      }
      if (chunk.done) {
        if (buffer.trim()) parse(buffer);
        break;
      }
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
