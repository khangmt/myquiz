import { useState } from "react";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiFailure } from "../api/client";
import { useMe } from "../auth/useMe";
import type { AdminUser } from "../generated/AdminUser";
import type { AdminUserPage } from "../generated/AdminUserPage";
import type { AuditPage } from "../generated/AuditPage";
import { formatDate } from "../reports/Reports";

/** `/admin`: users and the audit log (FR-AD-01, FR-AD-04). */
export function Admin() {
  const { t } = useTranslation();
  const me = useMe();
  const [tab, setTab] = useState<"users" | "audit">("users");
  if (me.data?.role !== "admin") return <p>{t("admin.notAllowed")}</p>;
  const tabClass = (on: boolean) =>
    `rounded-t-lg px-4 py-2 font-semibold ${on ? "bg-white text-indigo-700 shadow" : "text-slate-600"}`;
  return (
    <section className="flex flex-col gap-4">
      <h1 className="text-2xl font-bold">{t("admin.title")}</h1>
      <div role="tablist" className="flex gap-1 border-b border-slate-200">
        <button role="tab" aria-selected={tab === "users"} className={tabClass(tab === "users")} onClick={() => setTab("users")}>
          {t("admin.users")}
        </button>
        <button role="tab" aria-selected={tab === "audit"} className={tabClass(tab === "audit")} onClick={() => setTab("audit")}>
          {t("admin.audit")}
        </button>
      </div>
      {tab === "users" ? <Users /> : <Audit />}
    </section>
  );
}

function Pager({ page, pages, onPage }: { page: number; pages: number; onPage: (p: number) => void }) {
  const { t } = useTranslation();
  if (pages <= 1) return null;
  return (
    <div className="flex items-center justify-center gap-3">
      <button type="button" disabled={page <= 1} onClick={() => onPage(page - 1)} className="underline disabled:opacity-40">
        {t("common.previous")}
      </button>
      <span className="text-sm text-slate-600">{t("common.pageOf", { page, pages })}</span>
      <button type="button" disabled={page >= pages} onClick={() => onPage(page + 1)} className="underline disabled:opacity-40">
        {t("common.next")}
      </button>
    </div>
  );
}

function Users() {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const [q, setQ] = useState("");
  const [page, setPage] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const list = useQuery({
    queryKey: ["admin-users", q, page],
    queryFn: () => api<AdminUserPage>(`/admin/users?q=${encodeURIComponent(q)}&page=${page}`),
    placeholderData: keepPreviousData,
  });
  const change = useMutation({
    mutationFn: ({ u, action }: { u: AdminUser; action: "suspend" | "reactivate" }) =>
      api(`/admin/users/${u.id}/${action}`, { method: "POST" }),
    onSuccess: () => {
      setError(null);
      void qc.invalidateQueries({ queryKey: ["admin-users"] });
      void qc.invalidateQueries({ queryKey: ["admin-audit"] });
    },
    onError: (e) => setError(e instanceof ApiFailure ? e.code : "INTERNAL"),
  });
  const data = list.data;
  const pages = data ? Math.max(1, Math.ceil(data.total / data.pageSize)) : 1;

  return (
    <div className="flex flex-col gap-3">
      <input
        className="rounded-lg border border-slate-300 px-3 py-2"
        placeholder={t("admin.searchUsers")}
        aria-label={t("admin.searchUsers")}
        value={q}
        onChange={(e) => {
          setQ(e.target.value);
          setPage(1);
        }}
      />
      {error && (
        <p role="alert" className="text-red-700">
          {t(`errors.${error}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}
      <div className="overflow-x-auto rounded-lg bg-white shadow">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-slate-200 text-slate-600">
            <tr>
              <th className="px-3 py-2">{t("admin.email")}</th>
              <th className="px-3 py-2">{t("admin.name")}</th>
              <th className="px-3 py-2">{t("admin.joined")}</th>
              <th className="px-3 py-2 text-right">{t("admin.quizzes")}</th>
              <th className="px-3 py-2 text-right">{t("admin.games")}</th>
              <th className="px-3 py-2">{t("admin.status")}</th>
              <th className="px-3 py-2" />
            </tr>
          </thead>
          <tbody>
            {data?.items.map((u) => (
              <tr key={u.id} className="border-b border-slate-100 last:border-0">
                <td className="px-3 py-2">{u.email}</td>
                <td className="px-3 py-2">
                  {u.displayName}
                  {u.role === "admin" && <span className="ml-2 rounded bg-indigo-100 px-1.5 text-xs text-indigo-800">admin</span>}
                </td>
                <td className="whitespace-nowrap px-3 py-2">{formatDate(u.createdAt, i18n.language)}</td>
                <td className="px-3 py-2 text-right">{u.quizCount}</td>
                <td className="px-3 py-2 text-right">{u.gameCount}</td>
                <td className="px-3 py-2">
                  <span className={u.status === "suspended" ? "font-semibold text-red-700" : "text-green-700"}>
                    {t(`admin.statusName.${u.status}`)}
                  </span>
                </td>
                <td className="px-3 py-2 text-right">
                  {u.role !== "admin" &&
                    (u.status === "active" ? (
                      <button
                        type="button"
                        className="text-red-700 underline"
                        disabled={change.isPending}
                        onClick={() => {
                          if (window.confirm(t("admin.confirmSuspend", { email: u.email }))) change.mutate({ u, action: "suspend" });
                        }}
                      >
                        {t("admin.suspend")}
                      </button>
                    ) : (
                      <button type="button" className="underline" disabled={change.isPending} onClick={() => change.mutate({ u, action: "reactivate" })}>
                        {t("admin.reactivate")}
                      </button>
                    ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <Pager page={page} pages={pages} onPage={setPage} />
    </div>
  );
}

const ACTIONS = [
  "login",
  "login_failed",
  "user_suspended",
  "user_reactivated",
  "admin_granted",
  "admin_revoked",
  "quiz_deleted",
  "quiz_exported",
  "quiz_imported",
  "report_exported",
  "report_deleted",
  "retention_notice_sent",
  "quiz_deleted_retention",
];

function Audit() {
  const { t, i18n } = useTranslation();
  const [filters, setFilters] = useState({ user: "", action: "", from: "", to: "" });
  const [page, setPage] = useState(1);
  const params = new URLSearchParams({ ...filters, page: String(page) }).toString();
  const log = useQuery({
    queryKey: ["admin-audit", params],
    queryFn: () => api<AuditPage>(`/admin/audit?${params}`),
    placeholderData: keepPreviousData,
  });
  const data = log.data;
  const pages = data ? Math.max(1, Math.ceil(data.total / data.pageSize)) : 1;
  const set = (k: keyof typeof filters) => (e: { target: { value: string } }) => {
    setFilters({ ...filters, [k]: e.target.value });
    setPage(1);
  };
  const input = "rounded-lg border border-slate-300 px-3 py-2 text-sm";

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <input className={input} placeholder={t("admin.filterUser")} aria-label={t("admin.filterUser")} value={filters.user} onChange={set("user")} />
        <select className={input} aria-label={t("admin.filterAction")} value={filters.action} onChange={set("action")}>
          <option value="">{t("admin.allActions")}</option>
          {ACTIONS.map((a) => (
            <option key={a} value={a}>
              {a}
            </option>
          ))}
        </select>
        <label className="flex items-center gap-1 text-sm">
          {t("admin.from")}
          <input type="date" className={input} value={filters.from} onChange={set("from")} />
        </label>
        <label className="flex items-center gap-1 text-sm">
          {t("admin.to")}
          <input type="date" className={input} value={filters.to} onChange={set("to")} />
        </label>
      </div>
      {log.isError && (
        <p role="alert" className="text-red-700">
          {t("errors.INTERNAL")}
        </p>
      )}
      <div className="overflow-x-auto rounded-lg bg-white shadow">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-slate-200 text-slate-600">
            <tr>
              <th className="px-3 py-2">{t("admin.time")}</th>
              <th className="px-3 py-2">{t("admin.actor")}</th>
              <th className="px-3 py-2">{t("admin.action")}</th>
              <th className="px-3 py-2">{t("admin.target")}</th>
              <th className="px-3 py-2">{t("admin.details")}</th>
            </tr>
          </thead>
          <tbody>
            {data?.items.map((r) => (
              <tr key={r.id} className="border-b border-slate-100 align-top last:border-0">
                <td className="whitespace-nowrap px-3 py-2">{formatDate(r.at, i18n.language)}</td>
                <td className="px-3 py-2">{r.actorEmail ?? t("admin.system")}</td>
                <td className="px-3 py-2 font-mono text-xs">{r.action}</td>
                <td className="px-3 py-2 text-xs">
                  {r.targetType && `${r.targetType}: `}
                  <span className="font-mono">{r.targetId}</span>
                </td>
                <td className="max-w-md break-all px-3 py-2 font-mono text-xs text-slate-600">{JSON.stringify(r.details)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <Pager page={page} pages={pages} onPage={setPage} />
    </div>
  );
}
