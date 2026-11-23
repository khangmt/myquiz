// The editor's quiz state and the edits it allows, as pure functions so
// they are easy to test. Rules match the server (crates/engine/src/quiz.rs).

import type { OptionDoc } from "../generated/OptionDoc";
import type { QuestionDoc } from "../generated/QuestionDoc";
import type { QuestionKind } from "../generated/QuestionKind";
import type { QuizDoc } from "../generated/QuizDoc";
import type { SaveQuiz } from "../generated/SaveQuiz";

export const KINDS: QuestionKind[] = ["single", "multi", "true_false", "fill_blank"];
export const TIME_LIMITS = [5, 10, 20, 30, 45, 60, 90, 120, 180, 240];
export const POINTS = [0, 1000, 2000];
export const MAX_OPTIONS = 6;
export const MIN_OPTIONS = 2;
export const MAX_ACCEPTED = 5;
export const BLANK = "___";

/** What the editor holds: the save body plus image URLs for display. */
export type Draft = SaveQuiz & { coverMediaUrl: string | null };

export function fromDoc(doc: QuizDoc): Draft {
  return {
    title: doc.title,
    description: doc.description,
    coverMediaId: doc.coverMediaId,
    coverMediaUrl: doc.coverMediaUrl,
    visibility: doc.visibility,
    questions: doc.questions,
  };
}

/** The PUT body; display-only fields are dropped. */
export function toSave(d: Draft): SaveQuiz {
  return {
    title: d.title,
    description: d.description,
    coverMediaId: d.coverMediaId,
    visibility: d.visibility,
    questions: d.questions.map((q) => ({ ...q, mediaUrl: null })),
  };
}

export function newQuestion(kind: QuestionKind, labels: { true: string; false: string }): QuestionDoc {
  const base: QuestionDoc = {
    type: kind,
    text: "",
    mediaId: null,
    mediaUrl: null,
    timeLimitS: kind === "fill_blank" ? 30 : 20,
    points: 1000,
    options: [],
    acceptedAnswers: [],
    ignoreDiacritics: false,
  };
  return withKind(base, kind, labels);
}

/**
 * Changes a question's type, keeping what still makes sense: text, image,
 * time and points always; options between choice types; the accepted
 * answers become correct options when leaving fill-in-the-blank (FR-QZ-05).
 */
export function withKind(
  q: QuestionDoc,
  kind: QuestionKind,
  labels: { true: string; false: string },
): QuestionDoc {
  const next: QuestionDoc = { ...q, type: kind };
  if (kind === "fill_blank") {
    next.acceptedAnswers =
      q.acceptedAnswers.length > 0
        ? q.acceptedAnswers
        : q.options.filter((o) => o.correct && o.text.trim()).map((o) => o.text).slice(0, MAX_ACCEPTED);
    next.options = [];
    if (!next.text.includes(BLANK)) next.text = next.text ? `${next.text} ${BLANK}` : BLANK;
    return next;
  }
  next.acceptedAnswers = [];
  next.ignoreDiacritics = false;
  if (kind === "true_false") {
    const firstCorrect = q.options.findIndex((o) => o.correct);
    next.options = [
      { text: labels.true, correct: firstCorrect !== 1 },
      { text: labels.false, correct: firstCorrect === 1 },
    ];
    return next;
  }
  let options: OptionDoc[] =
    q.type === "fill_blank"
      ? q.acceptedAnswers.map((a) => ({ text: a, correct: true }))
      : q.type === "true_false"
        ? []
        : q.options;
  while (options.length < MIN_OPTIONS) options = [...options, { text: "", correct: false }];
  if (kind === "single") {
    // Keep only the first correct option.
    let seen = false;
    options = options.map((o) => {
      const correct = o.correct && !seen;
      if (o.correct) seen = true;
      return { ...o, correct };
    });
  }
  next.options = options;
  return next;
}

/** Marks option `i` correct; single and true/false allow only one. */
export function toggleCorrect(q: QuestionDoc, i: number): QuestionDoc {
  const only = q.type === "single" || q.type === "true_false";
  return {
    ...q,
    options: q.options.map((o, j) =>
      only ? { ...o, correct: j === i } : j === i ? { ...o, correct: !o.correct } : o,
    ),
  };
}

export function move<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length || from === to) return list;
  const copy = [...list];
  const [item] = copy.splice(from, 1);
  copy.splice(to, 0, item);
  return copy;
}

export function replaceAt<T>(list: T[], i: number, item: T): T[] {
  return list.map((x, j) => (j === i ? item : x));
}

export function removeAt<T>(list: T[], i: number): T[] {
  return list.filter((_, j) => j !== i);
}
