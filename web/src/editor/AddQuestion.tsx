import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { QuestionKind } from "../generated/QuestionKind";

const KINDS: QuestionKind[] = ["single", "multi", "true_false", "fill_blank"];

const kindBtn =
  "rounded-xl border-2 border-ink bg-white px-3 py-1.5 text-left text-sm font-bold shadow-[0_2px_0_var(--color-ink)] transition hover:bg-mint active:translate-y-px active:shadow-none";

/**
 * Adding a question: a row of buttons after the last question, and while
 * that row is off screen, a small "+ Add question" button in the corner
 * that opens the same choices only when asked. Nothing stays on top of the
 * question being written.
 */
export function AddQuestion({ onAdd }: { onAdd: (kind: QuestionKind) => void }) {
  const { t } = useTranslation();
  const row = useRef<HTMLDivElement>(null);
  const corner = useRef<HTMLDivElement>(null);
  const [rowVisible, setRowVisible] = useState(true);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const el = row.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(([e]) => setRowVisible(e.isIntersecting));
    io.observe(el);
    return () => io.disconnect();
  }, []);

  // The corner menu closes on Escape or a click anywhere else.
  useEffect(() => {
    if (!open) return;
    const key = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    const click = (e: MouseEvent) => {
      if (!corner.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("keydown", key);
    document.addEventListener("mousedown", click);
    return () => {
      document.removeEventListener("keydown", key);
      document.removeEventListener("mousedown", click);
    };
  }, [open]);

  useEffect(() => {
    if (rowVisible) setOpen(false);
  }, [rowVisible]);

  const add = (k: QuestionKind) => {
    setOpen(false);
    onAdd(k);
  };

  return (
    <>
      <div ref={row} className="flex flex-wrap items-center gap-2 rounded-2xl border-[3px] border-dashed border-ink/40 p-3">
        <span className="text-sm font-extrabold">{t("editor.addQuestion")}</span>
        {KINDS.map((k) => (
          <button key={k} type="button" onClick={() => add(k)} className={kindBtn}>
            + {t(`editor.kinds.${k}`)}
          </button>
        ))}
      </div>

      {!rowVisible && (
        <div ref={corner} className="fixed bottom-4 right-4 z-20 flex flex-col items-end gap-2">
          {open && (
            <div
              role="menu"
              aria-label={t("editor.addQuestion")}
              className="flex animate-pop flex-col gap-2 rounded-2xl border-[3px] border-ink bg-paper p-3 shadow-[0_4px_0_var(--color-ink)]"
            >
              {KINDS.map((k) => (
                <button key={k} type="button" role="menuitem" onClick={() => add(k)} className={kindBtn}>
                  + {t(`editor.kinds.${k}`)}
                </button>
              ))}
            </div>
          )}
          <button
            type="button"
            aria-expanded={open}
            aria-haspopup="menu"
            onClick={() => setOpen((o) => !o)}
            className="rounded-full border-[3px] border-ink bg-sun px-4 py-2 text-sm font-extrabold shadow-[0_3px_0_var(--color-ink)] transition active:translate-y-px active:shadow-none"
          >
            {open ? `✕ ${t("editor.close")}` : `+ ${t("editor.addQuestionShort")}`}
          </button>
        </div>
      )}
    </>
  );
}
