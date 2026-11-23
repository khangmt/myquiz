import { useTranslation } from "react-i18next";
import type { QuestionDoc } from "../generated/QuestionDoc";
import type { QuestionKind } from "../generated/QuestionKind";
import { ImageField } from "./ImageField";
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
      aria-labelledby={`${id}-title`}
      className={`flex flex-col gap-3 rounded-lg border bg-white p-4 ${problems.length ? "border-red-300" : "border-slate-200"}`}
    >
      <header className="flex flex-wrap items-center gap-2">
        <h2 id={`${id}-title`} className="mr-auto font-semibold">
          {t("editor.questionN", { n: index + 1 })}
        </h2>
        <select
          aria-label={t("editor.type")}
          value={q.type}
          onChange={(e) => onChange(withKind(q, e.target.value as QuestionKind, labels))}
          className="rounded border border-slate-300 px-2 py-1 text-sm"
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`editor.kinds.${k}`)}
            </option>
          ))}
        </select>
        <button type="button" className="px-1 text-sm underline disabled:opacity-40" disabled={index === 0} onClick={() => onMove(index - 1)} aria-label={t("editor.moveUp")}>
          ↑
        </button>
        <button type="button" className="px-1 text-sm underline disabled:opacity-40" disabled={index === count - 1} onClick={() => onMove(index + 1)} aria-label={t("editor.moveDown")}>
          ↓
        </button>
        <button type="button" className="text-sm underline" onClick={onDuplicate}>
          {t("quizzes.duplicate")}
        </button>
        <button type="button" className="text-sm text-red-700 underline" onClick={onDelete}>
          {t("quizzes.delete")}
        </button>
      </header>

      <label className="flex flex-col gap-1 text-sm">
        <span>{q.type === "fill_blank" ? t("editor.textWithBlank", { blank: BLANK }) : t("editor.text")}</span>
        <textarea
          value={q.text}
          maxLength={300}
          rows={2}
          onChange={(e) => set({ text: e.target.value })}
          className="rounded border border-slate-300 px-2 py-1 text-base"
        />
      </label>

      <ImageField
        url={q.mediaUrl}
        label={t("editor.addImage")}
        onChange={(m) => set({ mediaId: m?.id ?? null, mediaUrl: m?.url ?? null })}
      />

      <div className="flex flex-wrap gap-4 text-sm">
        <label className="flex items-center gap-2">
          {t("editor.time")}
          <select value={q.timeLimitS} onChange={(e) => set({ timeLimitS: Number(e.target.value) })} className="rounded border border-slate-300 px-2 py-1">
            {TIME_LIMITS.map((s) => (
              <option key={s} value={s}>
                {t("editor.seconds", { s })}
              </option>
            ))}
          </select>
        </label>
        <label className="flex items-center gap-2">
          {t("editor.points")}
          <select value={q.points} onChange={(e) => set({ points: Number(e.target.value) })} className="rounded border border-slate-300 px-2 py-1">
            {POINTS.map((p) => (
              <option key={p} value={p}>
                {t(`editor.pointsOption.${p}`)}
              </option>
            ))}
          </select>
        </label>
      </div>

      {choice && (
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-1 text-sm">{single ? t("editor.pickOne") : t("editor.pickAll")}</legend>
          {q.options.map((o, i) => (
            <div key={i} className="flex items-center gap-2">
              <input
                type={single ? "radio" : "checkbox"}
                name={`${id}-correct`}
                checked={o.correct}
                onChange={() => onChange(toggleCorrect(q, i))}
                aria-label={t("editor.correctN", { n: i + 1 })}
              />
              <input
                value={o.text}
                maxLength={120}
                readOnly={q.type === "true_false"}
                onChange={(e) => set({ options: replaceAt(q.options, i, { ...o, text: e.target.value }) })}
                placeholder={t("editor.optionN", { n: i + 1 })}
                aria-label={t("editor.optionN", { n: i + 1 })}
                className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1"
              />
              {q.type !== "true_false" && q.options.length > MIN_OPTIONS && (
                <button type="button" className="text-sm text-red-700" onClick={() => set({ options: removeAt(q.options, i) })} aria-label={t("editor.removeOptionN", { n: i + 1 })}>
                  ✕
                </button>
              )}
            </div>
          ))}
          {q.type !== "true_false" && q.options.length < MAX_OPTIONS && (
            <button type="button" className="self-start text-sm underline" onClick={() => set({ options: [...q.options, { text: "", correct: false }] })}>
              {t("editor.addOption")}
            </button>
          )}
        </fieldset>
      )}

      {q.type === "fill_blank" && (
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-1 text-sm">{t("editor.accepted")}</legend>
          {q.acceptedAnswers.map((a, i) => (
            <div key={i} className="flex items-center gap-2">
              <input
                value={a}
                maxLength={50}
                onChange={(e) => set({ acceptedAnswers: replaceAt(q.acceptedAnswers, i, e.target.value) })}
                aria-label={t("editor.acceptedN", { n: i + 1 })}
                className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1"
              />
              <button type="button" className="text-sm text-red-700" onClick={() => set({ acceptedAnswers: removeAt(q.acceptedAnswers, i) })} aria-label={t("editor.removeOptionN", { n: i + 1 })}>
                ✕
              </button>
            </div>
          ))}
          {q.acceptedAnswers.length < MAX_ACCEPTED && (
            <button type="button" className="self-start text-sm underline" onClick={() => set({ acceptedAnswers: [...q.acceptedAnswers, ""] })}>
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
        <ul role="alert" className="list-disc pl-5 text-sm text-red-800">
          {problems.map((p) => (
            <li key={p}>{t(`problems.${p}`, { defaultValue: p })}</li>
          ))}
        </ul>
      )}
    </article>
  );
}
