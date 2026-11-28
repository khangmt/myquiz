import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiFailure } from "../api/client";
import type { GameCreated } from "../generated/GameCreated";
import type { LibraryPage } from "../generated/LibraryPage";
import type { QuizDoc } from "../generated/QuizDoc";

export type Scope = "shared" | "public";

/** Other people's published quizzes, shared with me or public. They can be
 * hosted or copied into My quizzes, never edited. */
export function Library({ scope }: { scope: Scope }) {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const [notice, setNotice] = useState<{ kind: "copied"; id: string; title: string } | { kind: "error"; code: string } | null>(null);

  useEffect(() => {
    const id = setTimeout(() => {
      setQuery(search.trim());
      setPage(1);
    }, 300);
    return () => clearTimeout(id);
  }, [search]);
  useEffect(() => {
    setPage(1);
    setNotice(null);
  }, [scope]);

  const list = useQuery({
    queryKey: ["library", scope, query, page],
    queryFn: () => api<LibraryPage>(`/library?scope=${scope}&q=${encodeURIComponent(query)}&page=${page}`),
    placeholderData: keepPreviousData,
  });
  const fail = (e: unknown) => setNotice({ kind: "error", code: e instanceof ApiFailure ? e.code : "INTERNAL" });

  const host = useMutation({
    mutationFn: (id: string) => api<GameCreated>("/games", { method: "POST", json: { quizId: id } }),
    onSuccess: (game) => navigate(`/host/${game.pin}`),
    onError: fail,
  });
  const copy = useMutation({
    mutationFn: (id: string) => api<QuizDoc>(`/quizzes/${id}/duplicate`, { method: "POST" }),
    onSuccess: (doc) => {
      setNotice({ kind: "copied", id: doc.id, title: doc.title });
      void qc.invalidateQueries({ queryKey: ["quizzes"] });
    },
    onError: fail,
  });

  const date = (iso: string) => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" }).format(new Date(iso));
  const data = list.data;
  const pages = data ? Math.max(1, Math.ceil(data.total / data.pageSize)) : 1;

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-slate-600">{t(`library.${scope}Intro`)}</p>
      {notice?.kind === "copied" && (
        <p role="status" className="rounded-lg bg-green-50 px-3 py-2 text-green-900">
          {t("library.copied", { title: notice.title })}{" "}
          <Link to={`/quizzes/${notice.id}/edit`} className="font-semibold underline">
            {t("library.openCopy")}
          </Link>
        </p>
      )}
      {notice?.kind === "error" && (
        <p role="alert" className="rounded-lg bg-red-50 px-3 py-2 text-red-800">
          {t(`errors.${notice.code}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}
      <input
        type="search"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder={t("quizzes.search")}
        aria-label={t("quizzes.search")}
        className="w-full rounded-lg border border-slate-300 px-3 py-2"
      />
      {list.isPending && <p className="text-slate-500">{t("common.loading")}</p>}
      {list.isError && <p className="text-red-700">{t("errors.INTERNAL")}</p>}
      {data && data.items.length === 0 && (
        <p className="py-8 text-center text-slate-500">{query ? t("quizzes.noMatch") : t(`library.${scope}Empty`)}</p>
      )}
      <ul className="flex flex-col gap-2">
        {data?.items.map((q) => (
          <li key={q.id} className="flex flex-wrap items-center gap-3 rounded-lg border border-slate-200 bg-white p-3">
            {q.coverMediaUrl && <img src={q.coverMediaUrl} alt="" className="h-12 w-16 rounded object-cover" />}
            <div className="mr-auto min-w-0">
              <p className="font-semibold">{q.title}</p>
              {q.description && <p className="line-clamp-2 text-sm text-slate-700">{q.description}</p>}
              <p className="text-sm text-slate-600">
                {t("library.by", { name: q.ownerName })} · {t("quizzes.questions", { count: Number(q.questionCount) })} ·{" "}
                {t("library.updated", { date: date(q.updatedAt) })}
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <button
                type="button"
                disabled={host.isPending}
                className="rounded-lg bg-indigo-600 px-3 py-1.5 font-semibold text-white disabled:opacity-50"
                onClick={() => host.mutate(q.id)}
              >
                {host.isPending && host.variables === q.id ? t("quizzes.hosting") : t("quizzes.host")}
              </button>
              <button type="button" disabled={copy.isPending} className="underline disabled:opacity-50" onClick={() => copy.mutate(q.id)}>
                {t("library.copy")}
              </button>
            </div>
          </li>
        ))}
      </ul>
      {pages > 1 && (
        <div className="flex items-center justify-center gap-3">
          <button type="button" disabled={page <= 1} onClick={() => setPage(page - 1)} className="underline disabled:opacity-40">
            {t("common.previous")}
          </button>
          <span className="text-sm text-slate-600">{t("common.pageOf", { page, pages })}</span>
          <button type="button" disabled={page >= pages} onClick={() => setPage(page + 1)} className="underline disabled:opacity-40">
            {t("common.next")}
          </button>
        </div>
      )}
    </div>
  );
}
