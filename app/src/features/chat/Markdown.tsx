import { memo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";

/*
 * Model output is untrusted. Raw HTML is never rendered (react-markdown's
 * default), links don't navigate the app window, and images never load
 * (that would be a request to a host the user didn't choose).
 */
const components: Components = {
  a: ({ href, children }) => (
    <span className="text-beam underline decoration-dotted underline-offset-2" title={href}>
      {children}
      {href && typeof children === "string" && children !== href && (
        <span className="ml-1 text-ink-faint select-all">({href})</span>
      )}
    </span>
  ),
  img: ({ alt, src }) => (
    <span className="rounded bg-surface-3 px-1 text-ink-faint" title={src}>
      [image: {alt || "no description"}]
    </span>
  ),
  pre: ({ children }) => <pre className="my-3 overflow-x-auto rounded-lg border border-line bg-night p-3 text-[13px]">{children}</pre>,
  code: ({ className, children }) => (
    <code className={className ? `${className} font-mono` : "rounded bg-surface-3 px-1 py-0.5 font-mono text-[0.92em]"}>{children}</code>
  ),
  table: ({ children }) => (
    <div className="my-3 overflow-x-auto">
      <table className="border-collapse text-left">{children}</table>
    </div>
  ),
  th: ({ children }) => <th className="border border-line px-2 py-1 font-semibold">{children}</th>,
  td: ({ children }) => <td className="border border-line px-2 py-1">{children}</td>,
};

export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[[rehypeHighlight, { detect: false }]]} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
});
