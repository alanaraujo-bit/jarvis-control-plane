import { memo, useMemo, useState } from "react";
import { Check, Copy } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "../../app/platform";
import { useT } from "../../app/i18n";
import { parseMarkdown, type Block, type Inline } from "./markdown";

/**
 * An agent's reply, rendered from `parseMarkdown`'s data as React elements —
 * never as HTML (M29). Memoised: a long conversation re-renders on every poll,
 * and a reply that has not changed should not be parsed again.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  return (
    <div className="md selectable">
      {blocks.map((block, index) => (
        <BlockView key={index} block={block} />
      ))}
    </div>
  );
});

function BlockView({ block }: { block: Block }) {
  switch (block.type) {
    case "heading": {
      const Tag = block.level === 1 ? "h3" : block.level === 2 ? "h4" : "h5";
      return (
        <Tag className="md__heading" data-level={block.level}>
          <Inlines nodes={block.inline} />
        </Tag>
      );
    }
    case "paragraph":
      return (
        <p className="md__p">
          <Inlines nodes={block.inline} />
        </p>
      );
    case "list": {
      const items = block.items.map((item, index) => (
        <li key={index}>
          {item.map((child, childIndex) => (
            <BlockView key={childIndex} block={child} />
          ))}
        </li>
      ));
      return block.ordered ? (
        <ol className="md__list" start={block.start}>
          {items}
        </ol>
      ) : (
        <ul className="md__list">{items}</ul>
      );
    }
    case "quote":
      return (
        <blockquote className="md__quote">
          {block.blocks.map((child, index) => (
            <BlockView key={index} block={child} />
          ))}
        </blockquote>
      );
    case "code":
      return <CodeBlock lang={block.lang} text={block.text} />;
    case "table":
      return (
        <div className="md__table-wrap">
          <table className="md__table">
            <thead>
              <tr>
                {block.head.map((cell, index) => (
                  <th key={index}>
                    <Inlines nodes={cell} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  {row.map((cell, index) => (
                    <td key={index}>
                      <Inlines nodes={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "rule":
      return <hr className="md__rule" />;
  }
}

function Inlines({ nodes }: { nodes: Inline[] }) {
  return (
    <>
      {nodes.map((node, index) => {
        switch (node.type) {
          case "text":
            return <span key={index}>{node.text}</span>;
          case "code":
            return (
              <code key={index} className="md__code">
                {node.text}
              </code>
            );
          case "strong":
            return (
              <strong key={index}>
                <Inlines nodes={node.children} />
              </strong>
            );
          case "em":
            return (
              <em key={index}>
                <Inlines nodes={node.children} />
              </em>
            );
          case "link":
            return (
              <a
                key={index}
                className="md__link"
                href={node.href}
                // Never a navigation of this webview: the address opens in the
                // person's browser, and only http(s) ever reaches here.
                onClick={(event) => {
                  event.preventDefault();
                  void (isTauri() ? openUrl(node.href) : window.open(node.href, "_blank", "noopener"));
                }}
              >
                <Inlines nodes={node.children} />
              </a>
            );
        }
      })}
    </>
  );
}

function CodeBlock({ lang, text }: { lang: string; text: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  return (
    <figure className="md__pre">
      <figcaption className="md__pre-head">
        <span className="md__pre-lang">{lang}</span>
        <button
          type="button"
          className="md__copy"
          onClick={() => {
            void navigator.clipboard.writeText(text).then(() => {
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1400);
            });
          }}
          aria-label={copied ? t("chat.copied") : t("chat.copy")}
          title={copied ? t("chat.copied") : t("chat.copy")}
        >
          {copied ? (
            <Check size={12} strokeWidth={2.2} aria-hidden="true" />
          ) : (
            <Copy size={12} strokeWidth={2} aria-hidden="true" />
          )}
        </button>
      </figcaption>
      <pre>
        <code>{text}</code>
      </pre>
    </figure>
  );
}
