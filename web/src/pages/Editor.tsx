import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiFailure } from "../api/client";
import { AddQuestion } from "../editor/AddQuestion";
import { ImageField } from "../editor/ImageField";
import { fromDoc, move, newQuestion, removeAt, replaceAt, type Draft } from "../editor/model";
import { QuestionEditor } from "../editor/QuestionEditor";
import { useAutosave, type SaveState } from "../editor/useAutosave";
import { ProblemList } from "../components/ProblemList";
import type { ExportLink } from "../generated/ExportLink";
import type { GameCreated } from "../generated/GameCreated";
import type { QuizDoc } from "../generated/QuizDoc";
import type { QuizProblemDoc } from "../generated/QuizProblemDoc";
import { daysLeft } from "./QuizList";

const caption = "text-xs font-extrabold uppercase tracking-wide text-slate-500";

export function Editor() {
  const { id = "" } = useParams();
  const { t } = useTranslation();
  const doc = useQuery({
    queryKey: ["quiz", id],
    queryFn: () => api<QuizDoc>(`/quizzes/${id}`),
    staleTime: Infinity,
    gcTime: 0,
  });
  if (doc.isPending) return <p className="text-slate-500">{t("common.loading")}</p>;
  if (doc.isError) {
    const code = doc.error instanceof ApiFailure ? doc.error.code : "INTERNAL";
    return (
      <p role="alert" className="text-red-700">
        {t(`errors.${code}`, { defaultValue: t("errors.INTERNAL") })}
      </p>
    );
  }
  return <EditorForm key={doc.data.id} initial={doc.data} />;
}

function SaveBadge({ state, onRetry }: { state: SaveState; onRetry: () => void }) {
  const { t } = useTranslation();
  const text: Record<SaveState["kind"], string> = {
    saved: t("editor.saved"),
    pending: t("editor.unsaved"),
    saving: t("editor.saving"),
    conflict: t("editor.conflict"),
    invalid: t("editor.notSaved"),
    error: t("editor.saveFailed"),
  };
  const bad = state.kind === "conflict" || state.kind === "invalid" || state.kind === "error";
  return (
    <span role="status" className={`text-sm ${bad ? "text-red-700" : "text-slate-500"}`}>
      {text[state.kind]}
      {state.kind === "conflict" && (
        <button type="button" className="ml-2 underline" onClick={() => window.location.reload()}>
          {t("editor.reload")}
        </button>
      )}
      {state.kind === "error" && (
        <button type="button" className="ml-2 underline" onClick={onRetry}>
          {t("editor.retry")}
        </button>
      )}
    </span>
  );
}

function EditorForm({ initial }: { initial: QuizDoc }) {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [draft, setDraft] = useState<Draft>(() => fromDoc(initial));
  const [meta, setMeta] = useState({ status: initial.status, expiresAt: initial.expiresAt });
  const [problems, setProblems] = useState<QuizProblemDoc[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSaved = useCallback(
    (doc: QuizDoc) => {
      setMeta({ status: doc.status, expiresAt: doc.expiresAt });
      void qc.invalidateQueries({ queryKey: ["quizzes"] });
    },
    [qc],
  );
  const save = useAutosave(initial.id, initial.version, onSaved);
  const saveProblems = save.state.kind === "invalid" ? save.state.problems : [];
  const shown = problems.length ? problems : saveProblems;

  const update = (next: Draft) => {
    setDraft(next);
    setProblems([]);
    setMessage(null);
    save.change(next);
  };
  const setQuestions = (questions: Draft["questions"]) => update({ ...draft, questions });

  // A question just added or duplicated: once it renders, scroll to it and
  // put the cursor in its text so the host can type straight away.
  const [focusQuestion, setFocusQuestion] = useState<number | null>(null);
  useEffect(() => {
    if (focusQuestion === null) return;
    setFocusQuestion(null);
    const card = document.getElementById(`q${focusQuestion}`);
    if (!card) return;
    const calm = window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches;
    card.scrollIntoView?.({ behavior: calm ? "auto" : "smooth", block: "start" });
    card.querySelector("textarea")?.focus({ preventScroll: true });
  }, [focusQuestion]);
  const labels = { true: t("editor.true"), false: t("editor.false") };

  /** Saves now, then runs `action`; stops if the save failed. */
  async function afterSave(action: () => Promise<void>) {
    setBusy(true);
    setMessage(null);
    try {
      if (!(await save.flush())) return;
      await action();
    } catch (e) {
      if (e instanceof ApiFailure && e.code === "QUIZ_INVALID") {
        setProblems((e.body as { problems?: QuizProblemDoc[] }).problems ?? []);
      } else {
        setMessage(e instanceof ApiFailure ? e.code : "INTERNAL");
      }
    } finally {
      setBusy(false);
    }
  }

  const publish = () =>
    afterSave(async () => {
      const doc = await api<QuizDoc>(`/quizzes/${initial.id}/publish`, { method: "POST" });
      setMeta({ status: doc.status, expiresAt: doc.expiresAt });
      setMessage("PUBLISHED");
    });
  const download = () =>
    afterSave(async () => {
      const link = await api<ExportLink>(`/quizzes/${initial.id}/export`, { method: "POST" });
      window.location.assign(link.url);
    });
  const host = () =>
    afterSave(async () => {
      const game = await api<GameCreated>("/games", { method: "POST", json: { quizId: initial.id } });
      navigate(`/host/${game.pin}`);
    });

  const date = new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }).format(new Date(meta.expiresAt));
  const quizLevel = shown.filter((p) => p.question === null);
  const byQuestion = (i: number) => shown.filter((p) => p.question === i).map((p) => p.code);
  const published = meta.status === "published";

  return (
    <section className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Link to="/quizzes" className="text-sm underline">
          ← {t("nav.myQuizzes")}
        </Link>
        <span className="text-sm text-slate-600">{t(`quizzes.status.${meta.status}`)}</span>
        <SaveBadge state={save.state} onRetry={() => void save.flush()} />
        <div className="ml-auto flex flex-wrap gap-2">
          <button type="button" disabled={busy} onClick={download} className="rounded-lg border border-slate-300 bg-white px-3 py-2 font-semibold disabled:opacity-50">
            {t("quizzes.download")}
          </button>
          {!published && (
            <button type="button" disabled={busy} onClick={publish} className="rounded-lg border border-indigo-600 px-3 py-2 font-semibold text-indigo-700 disabled:opacity-50">
              {t("editor.publish")}
            </button>
          )}
          {published && (
            <button type="button" disabled={busy} onClick={host} className="rounded-lg bg-indigo-600 px-3 py-2 font-semibold text-white disabled:opacity-50">
              {t("editor.host")}
            </button>
          )}
        </div>
      </div>

      <p className={`rounded-lg px-3 py-2 text-sm ${daysLeft(meta.expiresAt) <= 7 ? "bg-amber-100 text-amber-950" : "bg-amber-50 text-amber-900"}`}>
        {t("editor.retention", { date })}
      </p>

      {message === "PUBLISHED" && (
        <p role="status" className="rounded-lg bg-green-50 px-3 py-2 text-green-900">
          {t("editor.publishedNow")}
        </p>
      )}
      {message && message !== "PUBLISHED" && (
        <p role="alert" className="rounded-lg bg-red-50 px-3 py-2 text-red-800">
          {t(`errors.${message}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}
      {shown.length > 0 && <ProblemList title={t("editor.fixBeforePublish")} problems={quizLevel.length ? quizLevel : shown} />}

      <div className="overflow-hidden rounded-3xl border-[3px] border-ink bg-white shadow-[0_4px_0_var(--color-ink)]">
        <h2 className="border-b-[3px] border-ink bg-sand px-4 py-2.5 text-lg font-extrabold">
          <span aria-hidden>📝 </span>
          {t("editor.quizDetails")}
        </h2>
        <div className="flex flex-col gap-3 p-4">
          <label className="flex flex-col gap-1.5">
            <span className={caption}>{t("editor.title")}</span>
            <input
              value={draft.title}
              maxLength={120}
              onChange={(e) => update({ ...draft, title: e.target.value })}
              className="rounded-2xl border-[3px] border-ink bg-paper px-3 py-2 text-2xl font-extrabold text-ink focus:outline-none focus:ring-4 focus:ring-lagoon/50"
            />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className={caption}>{t("editor.description")}</span>
            <textarea value={draft.description} maxLength={1000} rows={2} onChange={(e) => update({ ...draft, description: e.target.value })} className="rounded-lg border-2 border-slate-300 px-2 py-1 focus:border-ink focus:outline-none focus:ring-4 focus:ring-lagoon/40" />
          </label>
          <ImageField
            url={draft.coverMediaUrl}
            label={t("editor.addCover")}
            onChange={(m) => update({ ...draft, coverMediaId: m?.id ?? null, coverMediaUrl: m?.url ?? null })}
          />
          <label className="flex items-center gap-2 text-sm">
            {t("editor.visibility")}
            <select value={draft.visibility} onChange={(e) => update({ ...draft, visibility: e.target.value })} className="rounded-lg border-2 border-slate-300 bg-white px-2 py-1">
              <option value="private">{t("editor.private")}</option>
              <option value="public">{t("editor.public")}</option>
            </select>
          </label>
        </div>
      </div>

      <h2 className="mt-4 flex items-center gap-3 text-xl font-extrabold">
        {t("editor.questionsHeading", { count: draft.questions.length })}
        <span aria-hidden className="h-[3px] flex-1 rounded-full bg-ink/15" />
      </h2>

      <div className="flex flex-col gap-6">
        {draft.questions.map((q, i) => (
          <QuestionEditor
            key={i}
            index={i}
            count={draft.questions.length}
            question={q}
            problems={byQuestion(i)}
            onChange={(nq) => setQuestions(replaceAt(draft.questions, i, nq))}
            onMove={(to) => setQuestions(move(draft.questions, i, to))}
            onDuplicate={() => {
              setQuestions([...draft.questions.slice(0, i + 1), { ...q }, ...draft.questions.slice(i + 1)]);
              setFocusQuestion(i + 1);
            }}
            onDelete={() => {
              if (window.confirm(t("editor.confirmDeleteQuestion", { n: i + 1 }))) setQuestions(removeAt(draft.questions, i));
            }}
          />
        ))}
      </div>

      <AddQuestion
        onAdd={(k) => {
          setQuestions([...draft.questions, newQuestion(k, labels)]);
          setFocusQuestion(draft.questions.length);
        }}
      />
    </section>
  );
}
