import type { ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import "./markdown.css";
import { plainMath } from "./plainMath";

/** Links never navigate the window: Mizan's answers show where a link points, as text. */
function Link({ href, children }: { href?: string; children?: ReactNode }) {
  return (
    <span className="md-link" title={href}>
      {children}
    </span>
  );
}

/** Wide tables scroll inside their own box, never the whole thread. */
function Table({ children }: { children?: ReactNode }) {
  return (
    <div className="md-table" role="region" tabIndex={0}>
      <table>{children}</table>
    </div>
  );
}

const components: Components = {
  a: ({ href, children }) => <Link href={href}>{children}</Link>,
  table: ({ children }) => <Table>{children}</Table>,
  // Headings in a chat answer are section titles, not page titles: one quiet size.
  h1: ({ children }) => <h4 dir="auto">{children}</h4>,
  h2: ({ children }) => <h4 dir="auto">{children}</h4>,
  h3: ({ children }) => <h4 dir="auto">{children}</h4>,
  h4: ({ children }) => <h4 dir="auto">{children}</h4>,
  h5: ({ children }) => <h4 dir="auto">{children}</h4>,
  h6: ({ children }) => <h4 dir="auto">{children}</h4>,
  p: ({ children }) => <p dir="auto">{children}</p>,
  ul: ({ children }) => <ul dir="auto">{children}</ul>,
  ol: ({ children }) => <ol dir="auto">{children}</ol>,
  img: ({ alt }) => <span className="md-link">{alt}</span>,
};

/** Mizan's answers as GitHub-flavoured Markdown: headings, lists (nested), tables, rules, quotes,
 *  code. Raw HTML in the text stays text, never markup, and links never navigate. TeX formulas
 *  read as plain text (`plainMath`). */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {plainMath(text)}
      </ReactMarkdown>
    </div>
  );
}
