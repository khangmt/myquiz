import { useTranslation } from "react-i18next";
import type { QuestionDoc } from "../generated/QuestionDoc";
import type { QuestionKind } from "../generated/QuestionKind";
import { optionStyle } from "../game/ui";
import { MediaField } from "./MediaField";
import {
  BLANK,
  KINDS,
  MAX_ACCEPTED,
  MAX_OPTIONS,
  MIN_OPTIONS,
  POINTS,
  TIME_LIMITS,
  removeAt,
  replaceAt,
  toggleCorrect,
  withKind,
} from "./model";

// The editor's comic look, matching the game screens: ink outlines, soft
// paper fields, and each answer in the colour and shape players will see.
const field = "rounded-lg border-2 border-slate-300 bg-white focus:border-ink focus:outline-none focus:ring-4 focus:ring-lagoon/40";
const caption = "text-xs font-extrabold uppercase tracking-wide text-slate-500";
const tool = "rounded-lg border-2 border-ink/15 bg-white px-2 py-1 text-sm font-bold transition hover:bg-white/70 disabled:opacity-40";
const remove = "flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-sm font-bold text-coral hover:bg-blush/40";
const addMore = "self-start rounded-xl border-2 border-dashed border-slate-400 px-3 py-1.5 text-sm font-bold text-slate-600 hover:border-ink hover:text-ink";

type Props = {
  index: number;
  count: number;
  question: QuestionDoc;
  problems: string[];
  onChange: (q: QuestionDoc) => void;
  onMove: (to: number) => void;
  onDuplicate: () => void;
  onDelete: () => void;
};

export function QuestionEditor({ index, count, question: q, problems, onChange, onMove, onDuplicate, onDelete }: Props) {
  const { t } = useTranslation();
  const labels = { true: t("editor.true"), false: t("editor.false") };
  const id = `q${index}`;
  const set = (patch: Partial<QuestionDoc>) => onChange({ ...q, ...patch });
  const choice = q.type !== "fill_blank";
  const single = q.type === "single" || q.type === "true_false";

  return (
    <article
      id={id}
      aria-labelledby={`${id}-title`}
      className={`scroll-mt-4 overflow-hidden rounded-3xl border-[3px] bg-white shadow-[0_4px_0_var(--color-ink)] ${problems.length ? "border-coral" : "border-ink"}`}
    >
      <header className={`flex flex-wrap items-center gap-2 border-b-[3px] px-4 py-2.5 ${problems.length ? "border-coral bg-blush/40" : "border-ink bg-mint"}`}>
        <span aria-hidden className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border-[3px] border-ink bg-sun text-base font-extrabold">
          {index + 1}
        </span>
        <h2 id={`${id}-title`} className="mr-auto text-lg font-extrabold">
          {t("editor.questionN", { n: index + 1 })}
        </h2>
        <select
          aria-label={t("editor.type")}
          value={q.type}
          onChange={(e) => onChange(withKind(q, e.target.value as QuestionKind, labels))}
          className={`${field} px-2 py-1 text-sm font-semibold`}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`editor.kinds.${k}`)}
            </option>
          ))}
        </select>
        <button type="button" className={tool} disabled={index === 0} onClick={() => onMove(index - 1)} aria-label={t("editor.moveUp")}>
          ↑
        </button>
        <button type="button" className={tool} disabled={index === count - 1} onClick={() => onMove(index + 1)} aria-label={t("editor.moveDown")}>
          ↓
        </button>
        <button type="button" className={tool} onClick={onDuplicate}>
          {t("quizzes.duplicate")}
        </button>
        <button type="button" className={`${tool} text-coral hover:bg-blush/40`} onClick={onDelete}>
          {t("quizzes.delete")}
        </button>
      </header>

      <div className="flex flex-col gap-4 p-4">
        <label className="flex flex-col gap-1.5">
          <span className={caption}>{q.type === "fill_blank" ? t("editor.textWithBlank", { blank: BLANK }) : t("editor.text")}</span>
          <textarea
            value={q.text}
            maxLength={300}
            rows={2}
            onChange={(e) => set({ text: e.target.value })}
            className="rounded-2xl border-[3px] border-ink bg-paper px-3 py-2 text-lg font-bold text-ink focus:outline-none focus:ring-4 focus:ring-lagoon/50"
          />
        </label>

        <div className="flex flex-col gap-3 rounded-2xl bg-slate-50 p-3">
          <MediaField question={q} onChange={set} />
          <div className="flex flex-wrap gap-4 text-sm">
            <label className="flex items-center gap-2">
              <span aria-hidden>⏱</span>
              {t("editor.time")}
              <select value={q.timeLimitS} onChange={(e) => set({ timeLimitS: Number(e.target.value) })} className={`${field} px-2 py-1`}>
                {TIME_LIMITS.map((s) => (
                  <option key={s} value={s}>
                    {t("editor.seconds", { s })}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-2">
              <span aria-hidden>⭐</span>
              {t("editor.points")}
              <select value={q.points} onChange={(e) => set({ points: Number(e.target.value) })} className={`${field} px-2 py-1`}>
                {POINTS.map((p) => (
                  <option key={p} value={p}>
                    {t(`editor.pointsOption.${p}`)}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </div>

        {choice && (
          <fieldset className="flex flex-col gap-2 border-t-2 border-dashed border-slate-300 pt-3">
            <legend className={`${caption} float-left mb-2 w-full`}>{single ? t("editor.pickOne") : t("editor.pickAll")}</legend>
            <div className="grid gap-2 sm:grid-cols-2">
              {q.options.map((o, i) => (
                <div
                  key={i}
                  className={`flex items-center gap-2 rounded-2xl border-[3px] p-2 transition ${o.correct ? "border-leaf-deep bg-mint" : "border-slate-200 bg-slate-50"}`}
                >
                  <span aria-hidden className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border-2 border-ink text-lg leading-none ${optionStyle(i).bg}`}>
                    {optionStyle(i).shape}
                  </span>
                  <input
                    value={o.text}
                    maxLength={120}
                    readOnly={q.type === "true_false"}
                    onChange={(e) => set({ options: replaceAt(q.options, i, { ...o, text: e.target.value }) })}
                    placeholder={t("editor.optionN", { n: i + 1 })}
                    aria-label={t("editor.optionN", { n: i + 1 })}
                    className={`${field} min-w-0 flex-1 px-2 py-1.5 font-semibold`}
                  />
                  <label
                    className={`flex shrink-0 cursor-pointer items-center gap-1 rounded-full border-2 px-2 py-1 text-xs font-bold ${o.correct ? "border-leaf-deep bg-leaf-deep text-white" : "border-slate-300 bg-white text-slate-500"}`}
                  >
                    <input
                      type={single ? "radio" : "checkbox"}
                      name={`${id}-correct`}
                      checked={o.correct}
                      onChange={() => onChange(toggleCorrect(q, i))}
                      aria-label={t("editor.correctN", { n: i + 1 })}
                      className="h-3.5 w-3.5 accent-leaf-deep"
                    />
                    {t("editor.correct")}
                  </label>
                  {q.type !== "true_false" && q.options.length > MIN_OPTIONS && (
                    <button type="button" className={remove} onClick={() => set({ options: removeAt(q.options, i) })} aria-label={t("editor.removeOptionN", { n: i + 1 })}>
                      ✕
                    </button>
                  )}
                </div>
              ))}
            </div>
            {q.type !== "true_false" && q.options.length < MAX_OPTIONS && (
              <button type="button" className={addMore} onClick={() => set({ options: [...q.options, { text: "", correct: false }] })}>
                {t("editor.addOption")}
              </button>
            )}
          </fieldset>
        )}

        {q.type === "fill_blank" && (
          <fieldset className="flex flex-col gap-2 border-t-2 border-dashed border-slate-300 pt-3">
            <legend className={`${caption} float-left mb-2 w-full`}>{t("editor.accepted")}</legend>
            <div className="grid gap-2 sm:grid-cols-2">
              {q.acceptedAnswers.map((a, i) => (
                <div key={i} className="flex items-center gap-2 rounded-2xl border-[3px] border-leaf-deep bg-mint p-2">
                  <span aria-hidden className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border-2 border-ink bg-leaf-deep text-lg font-bold text-white">
                    ✓
                  </span>
                  <input
                    value={a}
                    maxLength={50}
                    onChange={(e) => set({ acceptedAnswers: replaceAt(q.acceptedAnswers, i, e.target.value) })}
                    aria-label={t("editor.acceptedN", { n: i + 1 })}
                    className={`${field} min-w-0 flex-1 px-2 py-1.5 font-semibold`}
                  />
                  <button type="button" className={remove} onClick={() => set({ acceptedAnswers: removeAt(q.acceptedAnswers, i) })} aria-label={t("editor.removeOptionN", { n: i + 1 })}>
                    ✕
                  </button>
                </div>
              ))}
            </div>
            {q.acceptedAnswers.length < MAX_ACCEPTED && (
              <button type="button" className={addMore} onClick={() => set({ acceptedAnswers: [...q.acceptedAnswers, ""] })}>
                {t("editor.addAccepted")}
              </button>
            )}
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={q.ignoreDiacritics} onChange={(e) => set({ ignoreDiacritics: e.target.checked })} />
              {t("editor.ignoreDiacritics")}
            </label>
          </fieldset>
        )}

        {problems.length > 0 && (
          <ul role="alert" className="list-disc rounded-2xl bg-blush/30 py-2 pl-8 pr-3 text-sm text-red-800">
            {problems.map((p) => (
              <li key={p}>{t(`problems.${p}`, { defaultValue: p })}</li>
            ))}
          </ul>
        )}
      </div>
    </article>
  );
}
