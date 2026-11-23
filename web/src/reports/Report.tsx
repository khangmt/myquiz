import { Fragment, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiFailure } from "../api/client";
import type { ExportLink } from "../generated/ExportLink";
import type { ReportFormat } from "../generated/ReportFormat";
import type { SessionReport } from "../generated/SessionReport";
import { formatDate } from "./Reports";

const pct = (x: number) => `${Math.round(x * 100)}%`;
const secs = (ms: number | null) => (ms == null ? "–" : `${(ms / 1000).toFixed(1)} s`);

/** `/reports/:id`: summary, questions and players of one game (FR-RP-02 to 05, 09). */
export function Report() {
  const { id = "" } = useParams();
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [open, setOpen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const report = useQuery({
    queryKey: ["session", id],
    queryFn: () => api<SessionReport>(`/sessions/${id}`),
    retry: false,
  });

  const download = useMutation({
    mutationFn: (format: ReportFormat) =>
      api<ExportLink>(`/sessions/${id}/exports`, { method: "POST", json: { format } }),
    onSuccess: (link) => window.location.assign(link.url),
    onError: (e) => setError(e instanceof ApiFailure ? e.code : "INTERNAL"),
  });
  const remove = useMutation({
    mutationFn: () => api(`/sessions/${id}`, { method: "DELETE" }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sessions"] });
      navigate("/reports");
    },
    onError: (e) => setError(e instanceof ApiFailure ? e.code : "INTERNAL"),
  });

  if (report.isLoading) return <p className="text-slate-500">{t("common.loading")}</p>;
  if (report.isError || !report.data) {
    const code = report.error instanceof ApiFailure ? report.error.code : "INTERNAL";
    return (
      <div className="flex flex-col gap-3">
        <p role="alert" className="text-red-700">
          {t(`errors.${code}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
        <Link to="/reports" className="underline">
          {t("reports.title")}
        </Link>
      </div>
    );
  }
  const r = report.data;
  const s = r.session;
  const base = "rounded-lg border px-3 py-2 text-sm font-semibold disabled:opacity-50";
  const button = `${base} border-slate-300 bg-white`;
  const primary = `${base} border-indigo-600 bg-indigo-600 text-white`;

  return (
    <section className="flex flex-col gap-6">
      <div className="flex flex-col gap-1">
        <Link to="/reports" className="text-sm text-slate-600 underline">
          ← {t("reports.title")}
        </Link>
        <h1 className="text-2xl font-bold">{s.quizTitle}</h1>
        <p className="text-slate-600">
          {formatDate(s.startedAt, i18n.language)}
          {!s.ended && <span className="ml-2 text-amber-700">{t("reports.notFinished")}</span>}
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        <button type="button" className={primary} disabled={download.isPending} onClick={() => download.mutate("xlsx")}>
          {t("reports.downloadXlsx")}
        </button>
        <button type="button" className={button} disabled={download.isPending} onClick={() => download.mutate("csv")}>
          {t("reports.downloadCsv")}
        </button>
        <button type="button" className={button} disabled={download.isPending} onClick={() => download.mutate("csv_summary")}>
          {t("reports.downloadCsvSummary")}
        </button>
        <button
          type="button"
          className="ml-auto text-sm text-red-700 underline"
          disabled={remove.isPending}
          onClick={() => {
            if (window.confirm(t("reports.confirmDelete"))) remove.mutate();
          }}
        >
          {t("reports.delete")}
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {t(`errors.${error}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}

      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {[
          [t("reports.players"), String(s.playerCount)],
          [t("reports.averageScore"), String(s.averageScore)],
          [t("reports.accuracy"), pct(r.accuracy)],
          [t("reports.averageTime"), secs(r.averageMs)],
        ].map(([k, v]) => (
          <div key={k} className="rounded-lg bg-white p-3 shadow">
            <dt className="text-sm text-slate-600">{k}</dt>
            <dd className="text-2xl font-bold">{v}</dd>
          </div>
        ))}
      </dl>
      <p className="text-sm text-slate-600">{t("reports.participation", { value: pct(r.participation) })}</p>

      <div className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">{t("reports.questions")}</h2>
        <div className="overflow-x-auto rounded-lg bg-white shadow">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 text-slate-600">
              <tr>
                <th className="px-3 py-2">#</th>
                <th className="px-3 py-2">{t("reports.question")}</th>
                <th className="px-3 py-2 text-right">{t("reports.answered")}</th>
                <th className="px-3 py-2 text-right">{t("reports.correct")}</th>
                <th className="px-3 py-2 text-right">{t("reports.averageTime")}</th>
              </tr>
            </thead>
            <tbody>
              {r.questions.map((q) => (
                <tr key={q.index} className="border-b border-slate-100 last:border-0">
                  <td className="px-3 py-2">{q.index + 1}</td>
                  <td className="px-3 py-2">
                    {q.text}
                    {q.hard && (
                      <span className="ml-2 rounded bg-red-100 px-2 py-0.5 text-xs font-semibold text-red-800">{t("reports.hard")}</span>
                    )}
                  </td>
                  <td className="px-3 py-2 text-right">{q.answered}</td>
                  <td className="px-3 py-2 text-right">{pct(q.accuracy)}</td>
                  <td className="px-3 py-2 text-right">{secs(q.averageMs)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">{t("reports.playerResults")}</h2>
        <div className="overflow-x-auto rounded-lg bg-white shadow">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 text-slate-600">
              <tr>
                <th className="px-3 py-2">{t("reports.rank")}</th>
                <th className="px-3 py-2">{t("reports.nickname")}</th>
                <th className="px-3 py-2 text-right">{t("reports.score")}</th>
                <th className="px-3 py-2 text-right">{t("reports.correctCount")}</th>
              </tr>
            </thead>
            <tbody>
              {r.players.map((p) => (
                <Fragment key={p.id}>
                  <tr className="border-b border-slate-100">
                    <td className="px-3 py-2">{p.kicked ? t("reports.removed") : p.rank}</td>
                    <td className="px-3 py-2">
                      <button
                        type="button"
                        aria-expanded={open === p.id}
                        className="text-indigo-700 underline"
                        onClick={() => setOpen(open === p.id ? null : p.id)}
                      >
                        {p.nickname}
                      </button>
                    </td>
                    <td className="px-3 py-2 text-right">{p.score}</td>
                    <td className="px-3 py-2 text-right">
                      {p.correctCount}/{p.answers.length}
                    </td>
                  </tr>
                  {open === p.id && (
                    <tr className="border-b border-slate-100 bg-slate-50">
                      <td colSpan={4} className="px-3 py-2">
                        <ol className="flex flex-col gap-1">
                          {p.answers.map((a) => (
                            <li key={a.questionIndex} className="flex flex-wrap gap-x-3">
                              <span className="font-semibold">{a.questionIndex + 1}.</span>
                              <span className={a.answer == null ? "text-slate-500" : a.correct ? "text-green-700" : "text-red-700"}>
                                {a.answer ?? t("reports.noAnswer")} {a.answer != null && (a.correct ? "✓" : "✗")}
                              </span>
                              <span className="text-slate-600">{secs(a.responseMs)}</span>
                              <span className="text-slate-600">+{a.points}</span>
                            </li>
                          ))}
                        </ol>
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  );
}
