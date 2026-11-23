import { useTranslation } from "react-i18next";

/** Screens that arrive in the next PRs (editor, join, host). */
export function Placeholder() {
  const { t } = useTranslation();
  return <p className="py-12 text-center text-slate-500">{t("common.comingSoon")}</p>;
}
