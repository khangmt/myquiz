import { Navigate, useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { googleSignInUrl } from "../api/client";
import { useMe } from "../auth/useMe";

/** Sign-in page. Google redirects back here with ?error=CODE on refusal. */
export function Login() {
  const { t } = useTranslation();
  const [params] = useSearchParams();
  const me = useMe();
  const next = params.get("next") ?? "/quizzes";
  const error = params.get("error");

  if (me.data) return <Navigate to={next.startsWith("/") ? next : "/quizzes"} replace />;

  return (
    <section className="mx-auto flex max-w-sm flex-col items-center gap-6 py-12 text-center">
      <h1 className="text-2xl font-bold">{t("login.title")}</h1>
      <p className="text-slate-600">{t("login.intro")}</p>
      {error && (
        <p role="alert" className="w-full rounded-lg bg-red-50 px-3 py-2 text-red-800">
          {t(`errors.${error}`, { defaultValue: t("errors.LOGIN_FAILED") })}
        </p>
      )}
      <a
        href={googleSignInUrl(next)}
        className="w-full rounded-lg border border-slate-300 bg-white px-4 py-3 font-semibold shadow-sm hover:bg-slate-50"
      >
        {t("home.signIn")}
      </a>
    </section>
  );
}
