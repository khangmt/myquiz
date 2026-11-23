import { useState } from "react";
import { useTranslation } from "react-i18next";
import { LANGUAGES } from "./i18n";

// Placeholder home screen. The join flow, sign-in and editor replace it in
// later PRs; for now it proves routing, i18n and Tailwind are wired up.
export default function App() {
  const { t, i18n } = useTranslation();
  const [pin, setPin] = useState("");

  return (
    <main className="min-h-screen bg-slate-50 text-slate-900 flex flex-col items-center justify-center gap-6 p-4">
      <h1 className="text-4xl font-bold">{t("app.name")}</h1>
      <p className="text-slate-600">{t("app.tagline")}</p>

      <form className="flex gap-2 w-full max-w-sm" onSubmit={(e) => e.preventDefault()}>
        <input
          className="flex-1 rounded-lg border border-slate-300 px-3 py-2 text-lg tracking-widest"
          inputMode="numeric"
          maxLength={6}
          placeholder={t("home.pinPlaceholder")}
          aria-label={t("home.pinPlaceholder")}
          value={pin}
          onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
        />
        <button className="rounded-lg bg-indigo-600 px-4 py-2 font-semibold text-white" type="submit">
          {t("home.join")}
        </button>
      </form>

      <label className="text-sm text-slate-600">
        {t("language")}{" "}
        <select
          className="ml-1 rounded border border-slate-300 px-1"
          value={i18n.language}
          onChange={(e) => {
            void i18n.changeLanguage(e.target.value);
            try {
              localStorage.setItem("lang", e.target.value);
            } catch {
              // ignore blocked storage
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
    </main>
  );
}
