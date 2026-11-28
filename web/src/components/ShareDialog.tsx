import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiFailure } from "../api/client";
import type { QuizSharing } from "../generated/QuizSharing";

/** Splits pasted text into addresses: commas, semicolons, spaces or lines. */
export function splitEmails(text: string): string[] {
  return text
    .split(/[\s,;]+/)
    .map((e) => e.trim().toLowerCase())
    .filter(Boolean);
}

type Props = {
  quizId: string;
  title: string;
  published: boolean;
  onClose: () => void;
};

/** Make a quiz public, or share it with people by email. Others can host
 * it or copy it; only the owner can edit it. */
export function ShareDialog({ quizId, title, published, onClose }: Props) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const key = ["sharing", quizId];
  const current = useQuery({ queryKey: key, queryFn: () => api<QuizSharing>(`/quizzes/${quizId}/sharing`) });
  const [isPublic, setPublic] = useState(false);
  const [emails, setEmails] = useState<string[]>([]);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (current.data) {
      setPublic(current.data.visibility === "public");
      setEmails(current.data.emails);
    }
  }, [current.data]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const add = () => {
    const more = splitEmails(input).filter((e) => !emails.includes(e));
    if (more.length) setEmails([...emails, ...more]);
    setInput("");
  };

  const save = useMutation({
    mutationFn: () => {
      // Whatever is still typed in the box counts too.
      const all = [...emails, ...splitEmails(input).filter((e) => !emails.includes(e))];
      return api<QuizSharing>(`/quizzes/${quizId}/sharing`, {
        method: "PUT",
        json: { visibility: isPublic ? "public" : "private", emails: all },
      });
    },
    onSuccess: (s) => {
      qc.setQueryData(key, s);
      void qc.invalidateQueries({ queryKey: ["quizzes"] });
      void qc.invalidateQueries({ queryKey: ["quiz", quizId] });
      onClose();
    },
    onError: (e) => {
      if (e instanceof ApiFailure && e.code === "EMAIL_INVALID") {
        const message = (e.body as { error?: { message?: string } } | null)?.error?.message ?? "";
        setError(t("share.emailInvalid", { email: message.replace(/^.*: /, "") }));
      } else setError(t(`errors.${e instanceof ApiFailure ? e.code : "INTERNAL"}`, { defaultValue: t("errors.INTERNAL") }));
    },
  });

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-ink/40 p-4"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div role="dialog" aria-modal="true" aria-labelledby="share-title" className="flex w-full max-w-lg flex-col gap-4 rounded-2xl bg-white p-5 shadow-xl">
        <h2 id="share-title" className="text-xl font-bold">
          {t("share.title", { title })}
        </h2>
        {current.isPending && <p className="text-slate-500">{t("common.loading")}</p>}
        {current.data && (
          <>
            <label className="flex items-start gap-3">
              <input type="checkbox" checked={isPublic} onChange={(e) => setPublic(e.target.checked)} className="mt-1 h-4 w-4" />
              <span>
                <span className="font-semibold">{t("share.public")}</span>
                <span className="block text-sm text-slate-600">{t("share.publicHint")}</span>
              </span>
            </label>

            <div className="flex flex-col gap-2">
              <span className="font-semibold">{t("share.people")}</span>
              <span className="text-sm text-slate-600">{t("share.peopleHint")}</span>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      add();
                    }
                  }}
                  placeholder={t("share.emailPlaceholder")}
                  aria-label={t("share.emailPlaceholder")}
                  className="min-w-0 flex-1 rounded-lg border border-slate-300 px-3 py-2"
                />
                <button type="button" onClick={add} className="rounded-lg border border-slate-300 bg-white px-3 py-2 font-semibold">
                  {t("share.add")}
                </button>
              </div>
              {emails.length > 0 && (
                <ul className="flex max-h-48 flex-col gap-1 overflow-y-auto">
                  {emails.map((e) => (
                    <li key={e} className="flex items-center gap-2 rounded-lg bg-slate-100 px-3 py-1.5 text-sm">
                      <span className="mr-auto break-all">{e}</span>
                      <button
                        type="button"
                        className="text-red-700 underline"
                        aria-label={t("share.removeEmail", { email: e })}
                        onClick={() => setEmails(emails.filter((x) => x !== e))}
                      >
                        {t("share.remove")}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <p className="rounded-lg bg-amber-50 px-3 py-2 text-sm text-amber-900">
              {!published && <>{t("share.draftNote")} </>}
              {t("share.answersNote")}
            </p>
          </>
        )}
        {(error || current.isError) && (
          <p role="alert" className="rounded-lg bg-red-50 px-3 py-2 text-red-800">
            {error ?? t("errors.INTERNAL")}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <button type="button" onClick={onClose} className="rounded-lg border border-slate-300 bg-white px-3 py-2 font-semibold">
            {t("share.cancel")}
          </button>
          <button
            type="button"
            disabled={!current.data || save.isPending}
            onClick={() => save.mutate()}
            className="rounded-lg bg-indigo-600 px-3 py-2 font-semibold text-white disabled:opacity-50"
          >
            {save.isPending ? t("share.saving") : t("share.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
