import { useState } from "react";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { SessionPage } from "../generated/SessionPage";

export function formatDate(iso: string, lang: string): string {
  return new Date(iso).toLocaleString(lang === "vi" ? "vi-VN" : "en-GB", { dateStyle: "medium", timeStyle: "short" });
}

/** `/reports`: the host's past games, newest first (FR-RP-01). */
export function Reports() {
  const { t, i18n } = useTranslation();
  const [page, setPage] = useState(1);
  const list = useQuery({
    queryKey: ["sessions", page],
    queryFn: () => api<SessionPage>(`/sessions?page=${page}`),
    placeholderData: keepPreviousData,
  });
  const data = list.data;
  const pages = data ? Math.max(1, Math.ceil(data.total / data.pageSize)) : 1;

  return (
    <section className="flex flex-col gap-4">
      <h1 className="text-2xl font-bold">{t("reports.title")}</h1>
      {list.isLoading && <p className="text-slate-500">{t("common.loading")}</p>}
      {list.isError && <p role="alert" className="text-red-700">{t("errors.INTERNAL")}</p>}
      {data && data.items.length === 0 && <p className="text-slate-600">{t("reports.empty")}</p>}
      {data && data.items.length > 0 && (
        <div className="overflow-x-auto rounded-lg bg-white shadow">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 text-slate-600">
              <tr>
                <th className="px-3 py-2">{t("reports.date")}</th>
                <th className="px-3 py-2">{t("reports.quiz")}</th>
                <th className="px-3 py-2 text-right">{t("reports.players")}</th>
                <th className="px-3 py-2 text-right">{t("reports.averageScore")}</th>
              </tr>
            </thead>
            <tbody>
              {data.items.map((s) => (
                <tr key={s.id} className="border-b border-slate-100 last:border-0">
                  <td className="whitespace-nowrap px-3 py-2">{formatDate(s.startedAt, i18n.language)}</td>
                  <td className="px-3 py-2">
                    <Link to={`/reports/${s.id}`} className="font-medium text-indigo-700 underline">
                      {s.quizTitle}
                    </Link>
                    {!s.ended && <span className="ml-2 text-xs text-amber-700">{t("reports.notFinished")}</span>}
                  </td>
                  <td className="px-3 py-2 text-right">{s.playerCount}</td>
                  <td className="px-3 py-2 text-right">{s.averageScore}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
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
