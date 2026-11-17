import { memo, type ReactNode } from "react";
import katex from "katex";
import "katex/dist/katex.min.css";

// Match code first so examples and commands remain literal.
const segments = /(`+)[\s\S]*?\1(?!`)|\\(?:\$|\\)|\$\$([\s\S]+?)\$\$|\\\[([\s\S]+?)\\\]|\\\(([^\n]+?)\\\)|(?<!\$)\$(?![\s$])((?:\\.|[^\n$])+?)(?<!\s)\$(?![\d$])/g;

export const ChatMath = memo(function ChatMath({ content }: { content: string }) {
  const parts: ReactNode[] = [];
  let end = 0;
  for (const match of content.matchAll(segments)) {
    const [raw, , dollars, brackets, parens, inline] = match;
    const start = match.index;
    parts.push(content.slice(end, start));
    end = start + raw.length;
    const formula = dollars ?? brackets ?? parens ?? inline;
    if (!formula?.trim()) {
      parts.push(raw);
      continue;
    }
    const display = dollars !== undefined || brackets !== undefined;
    try {
      const html = katex.renderToString(formula, {
        displayMode: display,
        trust: false,
        maxExpand: 200,
        maxSize: 20,
      });
      parts.push(
        <span
          className={display ? "chat-math-display" : "chat-math-inline"}
          key={start}
          dangerouslySetInnerHTML={{ __html: html }}
        />,
      );
    } catch {
      // A streamed or unsupported expression remains readable as source.
      parts.push(raw);
    }
  }
  parts.push(content.slice(end));
  return <p className="chat-message-content">{parts}</p>;
});
