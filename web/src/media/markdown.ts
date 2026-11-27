// Markdown for a question's media area: mostly a fenced code block (C for
// IT001), sometimes a short list or table. Rendered to HTML here and then
// sanitised with a strict allow-list, because quiz text comes from users and
// is shown on other people's screens.

import DOMPurify from "dompurify";
import { Marked, type Tokens } from "marked";
import hljs from "highlight.js/lib/core";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import python from "highlight.js/lib/languages/python";
import sql from "highlight.js/lib/languages/sql";
import bash from "highlight.js/lib/languages/bash";

hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("java", java);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("python", python);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("bash", bash);

/** Same limit as the server (crates/engine/src/quiz.rs MARKDOWN_MAX). */
export const MARKDOWN_MAX = 4000;

const escape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const marked = new Marked({
  gfm: true,
  breaks: false,
  renderer: {
    code({ text, lang }: Tokens.Code) {
      const language = (lang ?? "").trim().split(/\s+/)[0].toLowerCase();
      const known = language && hljs.getLanguage(language) ? language : "";
      const body = known ? hljs.highlight(text, { language: known, ignoreIllegals: true }).value : escape(text);
      return `<pre class="md-code"><code class="hljs${known ? ` language-${known}` : ""}">${body}</code></pre>`;
    },
    // Raw HTML in the source is shown as text, never as markup.
    html({ text }: Tokens.HTML | Tokens.Tag) {
      return escape(text);
    },
    // Images in Markdown would load from other sites; uploads use the
    // image option instead. Show the alt text.
    image({ text }: Tokens.Image) {
      return escape(text);
    },
    // Links are not followed from a quiz; show their text.
    link({ tokens }: Tokens.Link) {
      return this.parser.parseInline(tokens);
    },
  },
});

const ALLOWED_TAGS = [
  "p", "br", "hr", "strong", "em", "del", "code", "pre", "span", "blockquote",
  "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6",
  "table", "thead", "tbody", "tr", "th", "td", "input",
];

/** Markdown to sanitised HTML, safe to put in `dangerouslySetInnerHTML`. */
export function renderMarkdown(src: string): string {
  const html = marked.parse(src, { async: false });
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS,
    // class carries the highlighting; align comes from GFM tables; the
    // rest is for GFM task-list checkboxes.
    ALLOWED_ATTR: ["class", "align", "type", "checked", "disabled", "start"],
    ALLOW_DATA_ATTR: false,
  });
}
