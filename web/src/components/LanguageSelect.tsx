import { useTranslation } from "react-i18next";
import { LANGUAGES } from "../i18n";

export function LanguageSelect() {
  const { t, i18n } = useTranslation();
  return (
    <label className="text-sm text-slate-600">
      <span className="sr-only">{t("language")}</span>
      <select
        aria-label={t("language")}
        className="rounded border border-slate-300 bg-white px-1 py-0.5"
        value={i18n.language}
        onChange={(e) => {
          void i18n.changeLanguage(e.target.value);
          document.documentElement.lang = e.target.value;
          try {
            localStorage.setItem("lang", e.target.value);
          } catch {
            // storage can be blocked; the choice then lasts for this page only
          }
        }}
      >
        {LANGUAGES.map((l) => (
          <option key={l} value={l}>
            {l === "vi" ? "Tiếng Việt" : "English"}
          </option>
        ))}
      </select>
    </label>
  );
}
