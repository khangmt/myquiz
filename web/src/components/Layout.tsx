import { Link, Outlet, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../api/client";
import { ME_KEY, useMe } from "../auth/useMe";
import { ExpiryBanner } from "./ExpiryBanner";
import { LanguageSelect } from "./LanguageSelect";

export function Layout() {
  const { t } = useTranslation();
  const me = useMe();
  const qc = useQueryClient();
  const navigate = useNavigate();

  async function signOut() {
    try {
      await api("/auth/logout", { method: "POST" });
    } finally {
      qc.setQueryData(ME_KEY, null);
      navigate("/");
    }
  }

  return (
    <div className="min-h-screen bg-slate-50 text-slate-900">
      <header className="border-b border-slate-200 bg-white">
        <nav className="mx-auto flex max-w-5xl flex-wrap items-center gap-4 px-4 py-3">
          <Link to="/" className="text-xl font-bold text-indigo-700">
            {t("app.name")}
          </Link>
          {me.data && (
            <Link to="/quizzes" className="text-slate-700 hover:text-indigo-700">
              {t("nav.myQuizzes")}
            </Link>
          )}
          {me.data && (
            <Link to="/reports" className="text-slate-700 hover:text-indigo-700">
              {t("nav.reports")}
            </Link>
          )}
          {me.data?.role === "admin" && (
            <Link to="/admin" className="text-slate-700 hover:text-indigo-700">
              {t("nav.admin")}
            </Link>
          )}
          <div className="ml-auto flex items-center gap-3">
            <LanguageSelect />
            {me.data ? (
              <>
                <span className="hidden text-sm text-slate-600 sm:inline">{me.data.displayName}</span>
                <button type="button" onClick={signOut} className="text-sm text-slate-700 underline">
                  {t("nav.signOut")}
                </button>
              </>
            ) : (
              <Link to="/login" className="text-sm font-semibold text-indigo-700">
                {t("nav.signIn")}
              </Link>
            )}
          </div>
        </nav>
      </header>
      {me.data && <ExpiryBanner />}
      <main className="mx-auto max-w-5xl px-4 py-6">
        <Outlet />
      </main>
    </div>
  );
}
