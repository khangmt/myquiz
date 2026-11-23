import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";

/** The landing page: players type a PIN; creators sign in from the header. */
export function Home() {
  const { t } = useTranslation();
  const [pin, setPin] = useState("");
  const navigate = useNavigate();

  return (
    <section className="flex flex-col items-center gap-6 py-12 text-center">
      <h1 className="text-4xl font-bold">{t("app.name")}</h1>
      <p className="text-slate-600">{t("app.tagline")}</p>
      <form
        className="flex w-full max-w-sm gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (pin.length === 6) navigate(`/join?pin=${pin}`);
        }}
      >
        <input
          className="min-w-0 flex-1 rounded-lg border border-slate-300 px-3 py-2 text-lg tracking-widest"
          inputMode="numeric"
          autoComplete="off"
          maxLength={6}
          placeholder={t("home.pinPlaceholder")}
          aria-label={t("home.pinPlaceholder")}
          value={pin}
          onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
        />
        <button
          className="rounded-lg bg-indigo-600 px-4 py-2 font-semibold text-white disabled:opacity-50"
          type="submit"
          disabled={pin.length !== 6}
        >
          {t("home.join")}
        </button>
      </form>
    </section>
  );
}
