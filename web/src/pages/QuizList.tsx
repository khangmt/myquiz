import { useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiFailure } from "../api/client";
import { ProblemList } from "../components/ProblemList";
import type { ExportLink } from "../generated/ExportLink";
import type { GameCreated } from "../generated/GameCreated";
import type { Imported } from "../generated/Imported";
import type { ImportProblem } from "../generated/ImportProblem";
import type { QuizDoc } from "../generated/QuizDoc";
import type { QuizPage } from "../generated/QuizPage";
import type { QuizProblemDoc } from "../generated/QuizProblemDoc";
import type { QuizSummary } from "../generated/QuizSummary";

const DAY_MS = 86_400_000;

/** Days until the server deletes the quiz (FR-QZ-18), never below 0. */
export function daysLeft(expiresAt: string, now = Date.now()): number {
  return Math.max(0, Math.ceil((Date.parse(expiresAt) - now) / DAY_MS));
}

type Notice =
  | { kind: "importErrors"; problems: ImportProblem[] }
  | { kind: "exportErrors"; title: string; problems: QuizProblemDoc[] }
  | { kind: "hostErrors"; title: string; problems: QuizProblemDoc[] }
  | { kind: "error"; code: string }
  | { kind: "imported" };

export function QuizList() {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const [notice, setNotice] = useState<Notice | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  // Search after the user pauses typing.
  useEffect(() => {
    const id = setTimeout(() => {
      setQuery(search.trim());
      setPage(1);
    }, 300);
    return () => clearTimeout(id);
  }, [search]);

  const list = useQuery({
    queryKey: ["quizzes", query, page],
    queryFn: () => api<QuizPage>(`/quizzes?q=${encodeURIComponent(query)}&page=${page}`),
    placeholderData: keepPreviousData,
  });
  const refresh = () => qc.invalidateQueries({ queryKey: ["quizzes"] });
  const fail = (e: unknown) =>
    setNotice({ kind: "error", code: e instanceof ApiFailure ? e.code : "INTERNAL" });

  const create = useMutation({
    mutationFn: () => api<QuizDoc>("/quizzes", { method: "POST", json: { title: t("quizzes.untitled") } }),
    onSuccess: (q) => navigate(`/quizzes/${q.id}/edit`),
    onError: fail,
  });
  const duplicate = useMutation({
    mutationFn: (id: string) => api<QuizDoc>(`/quizzes/${id}/duplicate`, { method: "POST" }),
    onSuccess: refresh,
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api<void>(`/quizzes/${id}`, { method: "DELETE" }),
    onSuccess: refresh,
    onError: fail,
  });
  const download = useMutation({
    mutationFn: (q: QuizSummary) => api<ExportLink>(`/quizzes/${q.id}/export`, { method: "POST" }),
    onSuccess: (link) => {
      setNotice(null);
      window.location.assign(link.url);
    },
    onError: (e, q) => {
      if (e instanceof ApiFailure && e.code === "QUIZ_INVALID") {
        const problems = (e.body as { problems?: QuizProblemDoc[] }).problems ?? [];
        setNotice({ kind: "exportErrors", title: q.title, problems });
      } else fail(e);
    },
  });
  // "Host live" right from the list: a draft is published first, so one
  // click takes the lecturer to the lobby. Problems that block publishing
  // show here like the export ones do.
  const host = useMutation({
    mutationFn: async (q: QuizSummary) => {
      if (q.status !== "published") await api<QuizDoc>(`/quizzes/${q.id}/publish`, { method: "POST" });
      return api<GameCreated>("/games", { method: "POST", json: { quizId: q.id } });
    },
    onSuccess: (game) => navigate(`/host/${game.pin}`),
    onError: (e, q) => {
      if (e instanceof ApiFailure && e.code === "QUIZ_INVALID") {
        const problems = (e.body as { problems?: QuizProblemDoc[] }).problems ?? [];
        setNotice({ kind: "hostErrors", title: q.title, problems });
        void refresh();
      } else fail(e);
    },
  });
  const upload = useMutation({
    mutationFn: (file: File) => {
      const form = new FormData();
      form.append("file", file);
      return api<Imported>("/quizzes/import", { method: "POST", form });
    },
    onSuccess: () => {
      setNotice({ kind: "imported" });
      void refresh();
    },
    onError: (e) => {
      if (e instanceof ApiFailure && e.code === "IMPORT_INVALID") {
        const problems = (e.body as { errors?: ImportProblem[] }).errors ?? [];
        setNotice({ kind: "importErrors", problems });
      } else fail(e);
    },
  });

  const date = (iso: string) =>
    new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" }).format(new Date(iso));
  const data = list.data;
  const pages = data ? Math.max(1, Math.ceil(data.total / data.pageSize)) : 1;

  return (
    <section className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="mr-auto text-2xl font-bold">{t("quizzes.title")}</h1>
        <button
          type="button"
          onClick={() => fileInput.current?.click()}
          disabled={upload.isPending}
          className="rounded-lg border border-slate-300 bg-white px-3 py-2 font-semibold disabled:opacity-50"
        >
          {upload.isPending ? t("quizzes.uploading") : t("quizzes.upload")}
        </button>
        <input
          ref={fileInput}
          type="file"
          accept=".zip,application/zip"
          className="hidden"
          data-testid="zip-input"
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) upload.mutate(file);
          }}
        />
        <button
          type="button"
          onClick={() => create.mutate()}
          disabled={create.isPending}
          className="rounded-lg bg-indigo-600 px-3 py-2 font-semibold text-white disabled:opacity-50"
        >
          {t("quizzes.new")}
        </button>
      </div>

      <p className="rounded-lg bg-amber-50 px-3 py-2 text-sm text-amber-900">
        {t("quizzes.retention")}{" "}
        <a className="underline" href="/api/v1/quiz-template.zip">
          {t("quizzes.template")}
        </a>
      </p>

      {notice?.kind === "imported" && (
        <p role="status" className="rounded-lg bg-green-50 px-3 py-2 text-green-900">
          {t("quizzes.imported")}
        </p>
      )}
      {notice?.kind === "error" && (
        <p role="alert" className="rounded-lg bg-red-50 px-3 py-2 text-red-800">
          {t(`errors.${notice.code}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}
      {notice?.kind === "importErrors" && (
        <ProblemList title={t("quizzes.importFailed")} problems={notice.problems} />
      )}
      {notice?.kind === "exportErrors" && (
        <ProblemList title={t("quizzes.exportBlocked", { title: notice.title })} problems={notice.problems} />
      )}
      {notice?.kind === "hostErrors" && (
        <ProblemList title={t("quizzes.hostBlocked", { title: notice.title })} problems={notice.problems} />
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
        <p className="py-8 text-center text-slate-500">{query ? t("quizzes.noMatch") : t("quizzes.empty")}</p>
      )}

      <ul className="flex flex-col gap-2">
        {data?.items.map((q) => {
          const left = daysLeft(q.expiresAt);
          return (
            <li key={q.id} className="flex flex-wrap items-center gap-3 rounded-lg border border-slate-200 bg-white p-3">
              <div className="mr-auto min-w-0">
                <Link to={`/quizzes/${q.id}/edit`} className="font-semibold text-indigo-700 hover:underline">
                  {q.title}
                </Link>
                <p className="text-sm text-slate-600">
                  {t("quizzes.questions", { count: Number(q.questionCount) })} ·{" "}
                  {t(`quizzes.status.${q.status}`)} · {t("quizzes.deletedOn", { date: date(q.expiresAt) })}
                  {left <= 7 && (
                    <span className="ml-2 rounded bg-amber-100 px-1.5 py-0.5 text-xs font-semibold text-amber-900">
                      {t("quizzes.daysLeft", { count: left })}
                    </span>
                  )}
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <button
                  type="button"
                  disabled={host.isPending}
                  title={q.status === "published" ? undefined : t("quizzes.hostPublishes")}
                  className="rounded-lg bg-indigo-600 px-3 py-1.5 font-semibold text-white disabled:opacity-50"
                  onClick={() => host.mutate(q)}
                >
                  {host.isPending && host.variables?.id === q.id ? t("quizzes.hosting") : t("quizzes.host")}
                </button>
                <button type="button" className="underline" onClick={() => download.mutate(q)}>
                  {t("quizzes.download")}
                </button>
                <button type="button" className="underline" onClick={() => duplicate.mutate(q.id)}>
                  {t("quizzes.duplicate")}
                </button>
                <button
                  type="button"
                  className="text-red-700 underline"
                  onClick={() => {
                    if (window.confirm(t("quizzes.confirmDelete", { title: q.title }))) remove.mutate(q.id);
                  }}
                >
                  {t("quizzes.delete")}
                </button>
              </div>
            </li>
          );
        })}
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
    </section>
  );
}
