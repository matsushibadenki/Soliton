import { isValidElement, memo, useId, type ReactNode } from "react";
import { fromMarkdown } from "mdast-util-from-markdown";
import Markdown, { type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import katex from "katex";
import "katex/dist/katex.min.css";
import type { Language } from "./i18n";

type MarkdownNode = {
  type: string;
  position?: { start: { offset?: number }; end: { offset?: number } };
  children?: MarkdownNode[];
  value?: string;
};

// CommonMark treats LaTeX's backslash delimiters as punctuation escapes.
// Convert them before parsing while keeping literal Markdown regions intact.
const alternateMath =
  /(?<!\\)(?:\\\\)*?(\\\[([\s\S]+?)\\\]|\\\(([^\n]+?)\\\))|^[ \t]*\$\$([^\n$]+)\$\$[ \t]*(?=\r?$)/gm;

function normalizeMath(content: string) {
  if (!content.includes("\\[") && !content.includes("\\(") && !content.includes("$$")) {
    return content;
  }
  const matches = [...content.matchAll(alternateMath)];
  if (!matches.length) return content;
  const literal: { start: number; end: number }[] = [];
  function visit(node: MarkdownNode) {
    if (["code", "inlineCode", "html", "link", "image", "definition"].includes(node.type)) {
      const start = node.position?.start.offset;
      const end = node.position?.end.offset;
      if (start != null && end != null) literal.push({ start, end });
      return;
    }
    node.children?.forEach(visit);
  }
  visit(fromMarkdown(content));

  let previous = 0;
  let range = 0;
  const chunks: string[] = [];
  for (const match of matches) {
    const [raw, delimited, block, inline, dollars] = match;
    const start = match.index;
    const end = start + raw.length;
    while (range < literal.length && literal[range].end <= start) range++;
    if (range < literal.length && literal[range].start < end) continue;

    const formula = (block ?? inline ?? dollars)?.trim();
    if (!formula) continue;
    let fenceLength = 2;
    for (const [run] of formula.matchAll(/\$+/g)) fenceLength = Math.max(fenceLength, run.length + 1);
    const fence = "$".repeat(fenceLength);
    const prefix = delimited ? raw.slice(0, -delimited.length) : "";
    chunks.push(content.slice(previous, start), prefix);
    if (inline !== undefined) {
      chunks.push(fence, formula, fence);
    } else {
      chunks.push("\n", fence, "\n", formula, "\n", fence, "\n");
    }
    previous = end;
  }
  return chunks.join("") + content.slice(previous);
}

function MathExpression({ children, display }: { children: string; display: boolean }) {
  try {
    const html = katex.renderToString(children, {
      displayMode: display,
      trust: false,
      maxExpand: 200,
      maxSize: 20,
    });
    return (
      <span
        className={display ? "chat-math-display" : "chat-math-inline"}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  } catch {
    // Keep incomplete streamed formulas and unsupported TeX readable.
    return display ? <pre>{children}</pre> : <code>{children}</code>;
  }
}

function mathCode(node: ReactNode, className: string) {
  if (!isValidElement<{ className?: string; children?: ReactNode }>(node)) return null;
  if (node.type !== MarkdownCode || !node.props.className?.split(" ").includes(className)) return null;
  return typeof node.props.children === "string" ? node.props.children : null;
}

const MarkdownCode: NonNullable<Components["code"]> = ({ node: _node, className, children, ...props }) => {
  if (className?.split(" ").includes("math-inline") && typeof children === "string") {
    return <MathExpression display={false}>{children}</MathExpression>;
  }
  return <code className={className} {...props}>{children}</code>;
};

const components: Components = {
  code: MarkdownCode,
  pre: ({ children }) => {
    const formula = mathCode(children, "math-display");
    return formula === null ? <pre>{children}</pre> : <MathExpression display>{formula}</MathExpression>;
  },
  table: ({ node: _node, ...props }) => <div className="chat-table"><table {...props} /></div>,
  a: ({ node: _node, href, children }) => (
    <a href={href || undefined} target={href?.startsWith("#") ? undefined : "_blank"} rel="noopener noreferrer">
      {children}
    </a>
  ),
  img: ({ alt }) => <span>{alt}</span>,
};
function remarkCurrency() {
  return (tree: MarkdownNode, file: { value: string | Uint8Array }) => {
    const content = String(file.value);
    function visit(node: MarkdownNode) {
      node.children?.forEach((child, index, children) => {
        const start = child.position?.start.offset;
        const end = child.position?.end.offset;
        if (child.type === "inlineMath" && start != null && end != null) {
          const raw = content.slice(start, end);
          // For ordinary prices such as "$5 and $7", the second dollar sign
          // belongs to the next amount.
          if (/^\$\d/.test(raw) && /^\d/.test(content.slice(end, end + 1))) {
            children[index] = { type: "text", value: raw };
            return;
          }
        }
        visit(child);
      });
    }
    visit(tree);
  };
}
const plugins = [remarkGfm, remarkMath, remarkCurrency, remarkBreaks];
const footnotes = {
  en: { footnoteLabel: "Footnotes", footnoteBackLabel: "Back to reference" },
  ja: { footnoteLabel: "脚注", footnoteBackLabel: "参照に戻る" },
  zh: { footnoteLabel: "脚注", footnoteBackLabel: "返回引用" },
};

export const ChatMath = memo(function ChatMath({ content, language }: { content: string; language: Language }) {
  const footnotePrefix = "chat-" + useId().replace(/[^a-z0-9-]/gi, "") + "-";
  return (
    <div className="chat-message-content">
      <Markdown components={components} remarkPlugins={plugins} remarkRehypeOptions={{ ...footnotes[language], clobberPrefix: footnotePrefix }}>
        {normalizeMath(content)}
      </Markdown>
    </div>
  );
});
