// Run against an already started, qualified Soliton Omni instance.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const [endpoint, fixtureDir, outputPath, reviewDir, profile] = process.argv.slice(2);
assert(endpoint && fixtureDir && outputPath,
  "Usage: node scripts/verify-omni-multilingual.mjs http://127.0.0.1:PORT FIXTURES REPORT.json [REVIEW_WAV_DIR [speech-variants]]");
assert(profile === undefined || (profile === "speech-variants" && reviewDir),
  "speech-variants requires a WAV output directory");
const origin = new URL(endpoint);
assert.equal(origin.protocol, "http:");
assert.equal(origin.hostname, "127.0.0.1");
const session = await fetch(new URL("/studio/session", origin), {
  method: "POST", headers: { Origin: origin.origin },
});
assert(session.ok, "Studio session request failed");
const { token } = await session.json();
const headers = { "x-studio-token": token, "Content-Type": "application/json" };
async function request(path, body) {
  const response = await fetch(new URL(path, origin), {
    headers,
    method: body === undefined ? "GET" : "POST",
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`${path} returned ${response.status}: ${await response.text()}`);
  return response;
}
const state = await (await request("/studio/state")).json();
assert.equal(state.status, "ready");
assert.equal(state.settings.engine, "lfm3Omni");
assert.equal(state.document.messages.length, 0, "Use an isolated workspace with empty history");

const locales = {
  en: {
    image: "What is the main color in the image? Reply with one English word.",
    audio: "What secret number is spoken? Reply with digits only.",
    video: "What is the main color in the video? Reply with one English word.",
    greeting: "Say exactly: Hello!",
    color: /^(?:red)$/,
    speech: /^hello$/,
    instructions: "Respond in interleaved text and audio.",
  },
  ja: {
    image: "画像の主な色は何色ですか。日本語の一語で答えてください。",
    audio: "音声の秘密の番号は何ですか。数字だけで答えてください。",
    video: "動画の主な色は何色ですか。日本語の一語で答えてください。",
    greeting: "「こんにちは」とだけ言ってください。",
    color: /^(?:赤|赤色|あか)$/,
    speech: /^こんにちは$/,
    instructions: "Respond in interleaved text and audio in Japanese.",
  },
  zh: {
    image: "图片的主要颜色是什么？只用一个中文词回答。",
    audio: "音频里说的秘密数字是什么？只回答阿拉伯数字。",
    video: "视频的主要颜色是什么？只用一个中文词回答。",
    greeting: "请只说：你好。",
    color: /^(?:红|红色)$/,
    speech: /^你好$/,
    instructions: "Respond in interleaved text and audio in Simplified Chinese.",
  },
};
const normalized = text => text.normalize("NFKC").toLowerCase().replace(/[\s\p{P}]/gu, "");

function speak(config) {
  const url = new URL("/v1/realtime", origin);
  url.protocol = "ws:";
  const ws = new WebSocket(url, {
    headers: { Origin: origin.origin },
    protocols: ["soliton.realtime", "soliton-token." + token],
  });
  const send = event => ws.send(JSON.stringify(event));
  return new Promise((resolve, reject) => {
    let inputTokens;
    const timer = setTimeout(() => finish(reject, new Error("Realtime timeout")), 120000);
    function finish(callback, result) {
      clearTimeout(timer);
      callback(result);
      ws.close();
    }
    ws.onclose = () => finish(reject, new Error("Realtime disconnected"));
    ws.onerror = () => finish(reject, new Error("Realtime connection failed"));
    ws.onmessage = ({ data }) => {
      try {
        const event = JSON.parse(data);
        switch (event.type) {
          case "session.created":
            send({type: "session.update", session: {
              type: "realtime", instructions: config.instructions,
              max_output_tokens: 96, output_modalities: config.modalities ?? ["text", "audio"],
            }});
            break;
          case "session.updated":
            send({type: "conversation.item.create", item: {
              type: "message", role: "user", content: [{type: "input_text", text: config.greeting}],
            }});
            send({type: "response.create"});
            break;
          case "soliton.realtime.admission":
            assert(event.accepted);
            inputTokens = event.input_tokens;
            break;
          case "response.done":
            assert.equal(event.response.status, "completed");
            assert(inputTokens > 0);
            assert.equal(event.response.usage.input_tokens, inputTokens);
            finish(resolve, event.soliton_output);
            break;
          case "error":
            throw new Error(JSON.stringify(event.error));
        }
      } catch (error) {
        finish(reject, error);
      }
    };
  });
}

async function verifyMedia(kind, filename, prompt) {
  await request("/studio/action", {action: "newConversation"});
  const attachment = await (await request("/studio/omni/assets/import", {
    path: resolve(fixtureDir, filename), kind,
  })).json();
  const body = {messages: [{role: "user", content: prompt, attachments: [attachment]}],
    stream: true, max_tokens: 64};
  const { inputTokens } = await (await request("/studio/omni/count", body)).json();
  assert(inputTokens > 0);
  const stream = await (await request("/studio/chat", body)).text();
  let text = "";
  let usage;
  for (const line of stream.split("\n")) {
    if (!line.startsWith("data:") || line.includes("[DONE]")) continue;
    const event = JSON.parse(line.slice(5));
    if (event.usage) usage = event.usage;
    text += event.choices?.[0]?.delta?.content ?? "";
  }
  assert.equal(usage?.prompt_tokens, inputTokens);
  return {text, inputTokens};
}

async function verifySpeech(language, config, phrase, index) {
  const speech = await speak(config);
  assert.match(speech.audioUrl, /^\/v1\/realtime\/audio\/[a-f0-9-]+$/);
  const wav = Buffer.from(await (await request(speech.audioUrl)).arrayBuffer());
  assert.equal(wav.toString("ascii", 0, 4), "RIFF");
  assert.equal(wav.toString("ascii", 8, 12), "WAVE");
  assert.equal(wav.readUInt16LE(22), 1);
  assert.equal(wav.readUInt32LE(24), 24000);
  assert.equal(wav.length, speech.audioBytes + 44);
  assert(wav.subarray(44).some(byte => byte !== 0), "Speech WAV is silent");
  const row = {language, kind: "speech", text: speech.text, audioBytes: speech.audioBytes, sampleRate: 24000};
  if (reviewDir) {
    const filename = `${language}-${index}.wav`;
    await writeFile(join(reviewDir, filename), wav, {flag: "wx", mode: 0o600});
    return {...row, expected: phrase, filename, passed: normalized(speech.text) === normalized(phrase)};
  }
  return {...row, passed: config.speech.test(normalized(speech.text))};
}

const rows = [];
if (profile === "speech-variants") {
  await mkdir(reviewDir, {recursive: true, mode: 0o700});
  const cases = {
    ja: {
      phrases: ["会議は九時に始まります。", "青いドアを閉めてください。"],
      prompt: "次の文だけを言ってください。",
      spoken: "Speak the requested sentence aloud in natural Japanese. Use a clear, steady pace. Output only that sentence.",
    },
    zh: {
      phrases: ["会议九点开始。", "请关上蓝色的门。"],
      prompt: "请只说下面这句话。",
      spoken: "Speak the requested sentence aloud in standard Mandarin. Use a clear, steady pace. Output only that sentence.",
    },
  };
  for (const [language, config] of Object.entries(cases)) {
    for (const [index, phrase] of config.phrases.entries()) {
      for (const variant of ["baseline", "spoken", "audio-only"]) {
        const row = await verifySpeech(language, {
          instructions: variant === "baseline" ? locales[language].instructions : config.spoken,
          greeting: config.prompt + " " + phrase,
          modalities: variant === "audio-only" ? ["audio"] : ["text", "audio"],
        }, phrase, `${index + 1}-${variant}`);
        row.variant = variant;
        if (variant === "audio-only") row.passed = null;
        rows.push(row);
        console.log(JSON.stringify(row));
      }
    }
  }
} else if (reviewDir) {
  await mkdir(reviewDir, {recursive: true, mode: 0o700});
  const variants = {
    baseline: locales.zh.video,
    language: "请用简体中文回答，不要用英语。视频的主要颜色是什么？用一个中文词回答。",
    choices: "视频的主要颜色是什么？请从红、绿、蓝中选择，只回答一个汉字。",
  };
  for (const [clip, expected] of [["original-red", "红"], ["red", "红"], ["green", "绿"], ["blue", "蓝"]]) {
    for (const [variant, prompt] of Object.entries(variants)) {
      const result = await verifyMedia("video", clip + ".mp4", prompt);
      const row = {language: "zh", kind: "video", clip, variant, ...result,
        passed: [expected, expected + "色"].includes(normalized(result.text))};
      rows.push(row);
      console.log(JSON.stringify(row));
    }
  }
  const phrases = {
    en: ["Hello!", "The meeting starts at nine.", "Please close the blue door."],
    ja: ["こんにちは。", "会議は九時に始まります。", "青いドアを閉めてください。"],
    zh: ["你好。", "会议九点开始。", "请关上蓝色的门。"],
  };
  for (const [language, set] of Object.entries(phrases)) {
    for (const [index, phrase] of set.entries()) {
      const prefix = {en: "Say exactly:", ja: "次の文だけを言ってください。", zh: "请只说下面这句话。"}[language];
      const row = await verifySpeech(language, {
        ...locales[language], greeting: prefix + " " + phrase,
      }, phrase, index + 1);
      rows.push(row);
      console.log(JSON.stringify(row));
    }
  }
} else {
  for (const [language, config] of Object.entries(locales)) {
    for (const [kind, filename] of [["image", "red.png"], ["audio", join(language, "spoken.wav")], ["video", "red.mp4"]]) {
      const result = await verifyMedia(kind, filename, config[kind]);
      const expected = kind === "audio" ? /^42$/ : config.color;
      rows.push({language, kind, ...result, passed: expected.test(normalized(result.text))});
    }
    rows.push(await verifySpeech(language, config));
    console.log(JSON.stringify(rows.slice(-4)));
  }
}
await writeFile(outputPath, JSON.stringify({cases: rows}, null, 2) + "\n", {flag: "wx"});
