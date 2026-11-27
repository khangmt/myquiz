import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { QuestionDoc } from "../generated/QuestionDoc";
import { MARKDOWN_MAX } from "../media/markdown";
import { Markdown } from "../media/QuestionMedia";
import { ImageField } from "./ImageField";

type Mode = "image" | "markdown";

const STARTER = "```c\n\n```";

/**
 * A question's media area: an uploaded image, or Markdown such as a fenced
 * code block. Picking one clears the other, so players see one thing.
 */
export function MediaField({ question: q, onChange }: { question: QuestionDoc; onChange: (patch: Partial<QuestionDoc>) => void }) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<Mode>(q.markdown != null && !q.mediaId ? "markdown" : "image");

  function pick(m: Mode) {
    if (m === mode) return;
    setMode(m);
    // An empty box starts with a C block ready to fill in.
    if (m === "markdown") onChange({ mediaId: null, mediaUrl: null, markdown: q.markdown?.trim() ? q.markdown : STARTER });
    else onChange({ markdown: null });
  }

  const tab = (m: Mode, label: string) => (
    <button
      type="button"
      role="tab"
      aria-selected={mode === m}
      onClick={() => pick(m)}
      className={`rounded px-2 py-1 ${mode === m ? "bg-slate-800 text-white" : "text-slate-700 hover:bg-slate-100"}`}
    >
      {label}
    </button>
  );

  return (
    <div className="flex flex-col gap-2 text-sm">
      <div role="tablist" aria-label={t("editor.media.label")} className="flex items-center gap-1">
        <span className="mr-1 text-slate-600">{t("editor.media.label")}</span>
        {tab("image", t("editor.media.image"))}
        {tab("markdown", t("editor.media.code"))}
      </div>
      {mode === "image" ? (
        <ImageField url={q.mediaUrl} label={t("editor.addImage")} onChange={(m) => onChange({ mediaId: m?.id ?? null, mediaUrl: m?.url ?? null })} />
      ) : (
        <div className="grid gap-2 lg:grid-cols-2">
          <label className="flex flex-col gap-1">
            <span className="text-slate-600">{t("editor.media.codeHint")}</span>
            <textarea
              value={q.markdown ?? ""}
              maxLength={MARKDOWN_MAX}
              rows={10}
              spellCheck={false}
              placeholder={"```c\nint main(void) {\n    printf(\"Hello\\n\");\n}\n```"}
              aria-label={t("editor.media.code")}
              onChange={(e) => onChange({ markdown: e.target.value })}
              onKeyDown={(e) => {
                // Tab indents instead of leaving the box, as in a code editor.
                if (e.key !== "Tab" || e.shiftKey) return;
                e.preventDefault();
                const el = e.currentTarget;
                const { selectionStart: a, selectionEnd: b, value } = el;
                const next = value.slice(0, a) + "    " + value.slice(b);
                if (next.length > MARKDOWN_MAX) return;
                onChange({ markdown: next });
                requestAnimationFrame(() => el.setSelectionRange(a + 4, a + 4));
              }}
              className="rounded border border-slate-300 px-2 py-1 font-mono text-sm"
            />
            <span className="self-end text-xs text-slate-500 tabular-nums">
              {(q.markdown ?? "").length}/{MARKDOWN_MAX}
            </span>
          </label>
          <div className="flex min-w-0 flex-col gap-1">
            <span className="text-slate-600">{t("editor.media.preview")}</span>
            <div className="max-h-72 min-h-24 overflow-auto rounded border border-slate-200 bg-white px-3 py-2" data-testid="markdown-preview">
              {q.markdown?.trim() ? <Markdown source={q.markdown} className="md-editor" /> : <span className="text-slate-400">{t("editor.media.empty")}</span>}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
