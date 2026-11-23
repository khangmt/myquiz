import { useState } from "react";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { ExpiringQuiz } from "../generated/ExpiringQuiz";
import { daysLeft } from "../pages/QuizList";

const DISMISS_KEY = "myquiz.expiryBannerDismissed";

function dismissedToday(): boolean {
  try {
    return localStorage.getItem(DISMISS_KEY) === new Date().toDateString();
  } catch {
    return false;
  }
}

/** Shown to signed-in users when a quiz will be deleted within 3 days (FR-QZ-19). */
export function ExpiryBanner() {
  const { t } = useTranslation();
  const [hidden, setHidden] = useState(dismissedToday);
  const list = useQuery({
    queryKey: ["quizzes", "expiring"],
    queryFn: () => api<ExpiringQuiz[]>("/quizzes/expiring"),
    staleTime: 5 * 60_000,
    enabled: !hidden,
  });
  const items = list.data ?? [];
  if (items.length === 0 || hidden) return null;
  const soonest = daysLeft(items[0].expiresAt);

  return (
    <div role="status" className="border-b border-amber-300 bg-amber-50">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2 text-sm text-amber-900">
        <span>
          {t("expiry.banner", { count: items.length, days: soonest })}{" "}
          <strong>{items.map((q) => q.title).join(", ")}</strong>
        </span>
        <span>{t("expiry.howToKeep")}</span>
        <Link to="/quizzes" className="font-semibold underline">
          {t("nav.myQuizzes")}
        </Link>
        <button
          type="button"
          className="ml-auto underline"
          onClick={() => {
            try {
              localStorage.setItem(DISMISS_KEY, new Date().toDateString());
            } catch {
              /* private mode: the banner just stays */
            }
            setHidden(true);
          }}
        >
          {t("expiry.dismiss")}
        </button>
      </div>
    </div>
  );
}
