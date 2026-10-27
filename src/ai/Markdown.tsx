import { chatText, contentLanguage, isolateRuns } from "./content";
import { Children, isValidElement, cloneElement, memo, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import "./markdown.css";
import { withoutBody } from "../shell/methodName";
import { plainMath } from "./plainMath";

/** Links never navigate the window: Mizan's answers show where a link points, as text. */
function Link({ children }: { href?: string; children?: ReactNode }) {
  return (
    <span className="md-link">
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

function isolated(children: ReactNode): ReactNode {
  return Children.map(children, (child) => typeof child === "string" ? isolateRuns(child)
    : isValidElement<{ children?: ReactNode }>(child) && child.type !== "bdi" && child.type !== "code" ? cloneElement(child, {}, isolated(child.props.children)) : child);
}

function prose(children: ReactNode): string {
  return Children.toArray(children).map((child) => typeof child === "string" || typeof child === "number" ? String(child)
    : isValidElement<{ children?: ReactNode }>(child) ? prose(child.props.children) : "").join(" ");
}

function readingDirection(children: ReactNode) {
  const lang = contentLanguage(prose(children));
  return { lang, dir: lang === "ar" ? "rtl" : "ltr" };
}

const components: Components = {
  a: ({ href, children }) => <Link href={href}>{children}</Link>,
  table: ({ children }) => <Table>{children}</Table>,
  // Headings in a chat answer are section titles, not page titles: one quiet size.
  h1: ({ children }) => <h4>{isolated(children)}</h4>,
  h2: ({ children }) => <h4>{isolated(children)}</h4>,
  h3: ({ children }) => <h4>{isolated(children)}</h4>,
  h4: ({ children }) => <h4>{isolated(children)}</h4>,
  h5: ({ children }) => <h4>{isolated(children)}</h4>,
  h6: ({ children }) => <h4>{isolated(children)}</h4>,
  p: ({ children }) => <p>{isolated(children)}</p>,
  ul: ({ children }) => <ul>{children}</ul>,
  ol: ({ children }) => <ol>{children}</ol>,
  li: ({ children }) => <li {...readingDirection(children)}>{isolated(children)}</li>,
  th: ({ children }) => <th {...readingDirection(children)}>{isolated(children)}</th>,
  td: ({ children }) => <td {...readingDirection(children)}>{isolated(children)}</td>,
  code: ({ children }) => <code dir="auto">{children}</code>,
  img: ({ alt }) => <span className="md-link">{alt}</span>,
};

/** Mizan's answers as GitHub-flavoured Markdown: headings, lists (nested), tables, rules, quotes,
 *  code. Raw HTML in the text stays text, never markup, and links never navigate. TeX formulas
 *  read as plain text (`plainMath`). Memoised: parsing an answer is about a millisecond, and the
 *  thread has one per message; the app renders on every pointer move while a tool is in use. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  const lang = contentLanguage(text);
  return (
    <div className="md" lang={lang} dir={lang === "ar" ? "rtl" : "ltr"}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {withoutBody(plainMath(chatText(text)))}
      </ReactMarkdown>
    </div>
  );
});
