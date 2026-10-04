import { isValidElement, useMemo, useState, type ReactNode } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { api, errorMessage } from "../api";

function codeText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(codeText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return codeText(node.props.children);
  return "";
}

function CodeBlock({
  children,
  onError
}: {
  children: ReactNode;
  onError: (message: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const code = codeText(children);
  const className = isValidElement<{ className?: string }>(children)
    ? children.props.className ?? "" : "";
  const language = /(?:^|\s)language-([\w+-]+)/.exec(className)?.[1] ?? "text";

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
    } catch (reason) {
      onError(`Could not copy code: ${errorMessage(reason)}`);
    }
  }

  return (
    <div className="my-3 min-w-0 max-w-full overflow-hidden rounded-lg border border-white/10 bg-canvas">
      <div className="flex items-center justify-between border-b border-white/10 px-3 py-1 text-xs text-slate-400">
        <span>{language}</span>
        <button type="button" className="toolbar-button" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy code"}
        </button>
      </div>
      <pre className="max-h-[28rem] overflow-auto p-3" aria-label={`${language} code block`}>
        <code className="font-mono text-xs leading-relaxed text-slate-100">{code}</code>
      </pre>
    </div>
  );
}

function safeHttpUrl(value: string): string {
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:" ? url.href : "";
  } catch {
    return "";
  }
}

export function MarkdownMessage({
  content,
  onError
}: {
  content: string;
  onError: (message: string) => void;
}) {
  const components = useMemo<Components>(() => ({
    pre: ({ children }) => <CodeBlock onError={onError}>{children}</CodeBlock>,
    code: ({ children, className }) =>
      <code className={className ?? "rounded bg-raised px-1 py-0.5 font-mono text-xs text-accent"}>{children}</code>,
    a: ({ href, children }) => {
      const url = href ? safeHttpUrl(href) : "";
      return url ? (
        <a href={url} className="text-accent underline underline-offset-2"
          onClick={(event) => {
            event.preventDefault();
            void api.openUrl(url).catch((reason: unknown) => onError(errorMessage(reason)));
          }}>{children}</a>
      ) : <span>{children}</span>;
    },
    img: ({ alt }) => <span className="text-slate-400">[Image not loaded: {alt ?? "image"}]</span>,
    table: ({ children }) => (
      <div className="my-3 max-w-full overflow-x-auto rounded border border-white/10">
        <table className="w-full min-w-max text-left text-sm">{children}</table>
      </div>
    ),
    th: ({ children }) => <th className="border-b border-white/10 bg-raised px-3 py-2 font-semibold">{children}</th>,
    td: ({ children }) => <td className="border-b border-white/10 px-3 py-2">{children}</td>
  }), [onError]);

  return (
    <div className="chat-markdown min-w-0 break-words text-sm leading-relaxed">
      <Markdown remarkPlugins={[remarkGfm]} skipHtml urlTransform={safeHttpUrl} components={components}>
        {content}
      </Markdown>
    </div>
  );
}
