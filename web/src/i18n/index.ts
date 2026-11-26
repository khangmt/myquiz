import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import vi from "./vi.json";
import en from "./en.json";

// English is the default UI language; Vietnamese is the second bundle (NFR-11).
export const LANGUAGES = ["vi", "en"] as const;
export type Language = (typeof LANGUAGES)[number];

function initialLanguage(): Language {
  try {
    const saved = localStorage.getItem("lang");
    if (saved === "vi" || saved === "en") return saved;
  } catch {
    // storage can be blocked; fall through to the default
  }
  return "en";
}

void i18n.use(initReactI18next).init({
  resources: { vi: { translation: vi }, en: { translation: en } },
  lng: initialLanguage(),
  fallbackLng: "en",
  interpolation: { escapeValue: false }, // React already escapes output
});

export default i18n;
