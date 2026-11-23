import type { ReactNode } from "react";
import { Navigate, useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMe } from "./useMe";

/** Shows the page only to a signed-in user; others go to /login. */
export function RequireAuth({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const me = useMe();
  const location = useLocation();
  if (me.isPending) return <p className="p-8 text-center text-slate-500">{t("common.loading")}</p>;
  if (me.isError) return <p className="p-8 text-center text-red-700">{t("errors.INTERNAL")}</p>;
  if (!me.data) {
    const next = location.pathname + location.search;
    return <Navigate to={`/login?next=${encodeURIComponent(next)}`} replace />;
  }
  return <>{children}</>;
}
