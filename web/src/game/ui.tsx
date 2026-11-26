import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ScoreRow } from "../generated/ScoreRow";
import { Avatar, type Theme } from "./avatars";

// Each option has a colour and a shape, so colour-blind players can still
// tell them apart (NFR-12).
// Bold text on these fills meets WCAG AA for large text.
export const OPTION_STYLES = [
  { bg: "bg-coral text-white", shape: "▲" },
  { bg: "bg-sea text-white", shape: "◆" },
  { bg: "bg-sun text-ink", shape: "●" },
  { bg: "bg-leaf-deep text-white", shape: "■" },
  { bg: "bg-berry text-white", shape: "★" },
  { bg: "bg-lagoon text-ink", shape: "⬟" },
];

/** The comic look: a chunky ink outline with a small drop below. */
export const outline = "border-[3px] border-ink shadow-[0_4px_0_var(--color-ink)]";

export function optionStyle(i: number) {
  return OPTION_STYLES[i % OPTION_STYLES.length];
}

/** The option's shape in a soft badge. */
export function Shape({ i, large }: { i: number; large?: boolean }) {
  return (
    <span
      aria-hidden
      className={`flex shrink-0 items-center justify-center rounded-lg border-2 border-current bg-white/25 leading-none ${large ? "h-12 w-12 text-2xl" : "h-9 w-9 text-lg"}`}
    >
      {optionStyle(i).shape}
    </span>
  );
}

const press = "transition active:translate-y-[3px] active:shadow-[0_1px_0_var(--color-ink)] disabled:cursor-not-allowed disabled:opacity-40 disabled:active:translate-y-0";

export const btn = {
  primary: `inline-flex items-center justify-center rounded-2xl border-[3px] border-ink bg-coral px-6 py-3 text-lg font-extrabold text-white shadow-[0_4px_0_var(--color-ink)] hover:brightness-105 ${press}`,
  secondary: `inline-flex items-center justify-center rounded-2xl border-[3px] border-ink bg-white px-5 py-3 font-bold text-ink shadow-[0_4px_0_var(--color-ink)] hover:bg-mint ${press}`,
  ghost: "inline-flex items-center justify-center rounded-xl px-4 py-3 text-sm font-bold text-ink/60 transition hover:bg-ink/5 hover:text-ink",
  danger: `inline-flex items-center justify-center rounded-xl border-[3px] border-ink bg-coral px-4 py-2.5 text-sm font-bold text-white shadow-[0_3px_0_var(--color-ink)] ${press}`,
};

export const panel = "rounded-3xl border-[3px] border-ink bg-white/85 shadow-[0_5px_0_var(--color-ink)]";

export type Tone = "correct" | "wrong" | "missed";

/**
 * The full-screen game stage: a slim status bar, the content, and an
 * optional bottom bar that keeps the host's controls out of the audience's
 * way.
 */
export function Stage({
  left,
  center,
  right,
  footer,
  tone,
  narrow,
  theme,
  children,
}: {
  left?: ReactNode;
  center?: ReactNode;
  right?: ReactNode;
  footer?: ReactNode;
  tone?: Tone;
  narrow?: boolean;
  theme?: Theme;
  children: ReactNode;
}) {
  const width = narrow ? "max-w-md" : "max-w-6xl";
  return (
    <div className="game-stage flex min-h-dvh flex-col text-ink" data-tone={tone} data-theme={theme}>
      <header className="border-b-[3px] border-ink/10">
        <div className={`mx-auto flex min-h-14 w-full items-center gap-3 px-4 py-2 ${width}`}>
          <div className="flex min-w-0 flex-1 items-center gap-3">{left ?? <Logo />}</div>
          {center && <div className="shrink-0 text-sm font-bold text-ink/70">{center}</div>}
          <div className="flex min-w-0 flex-1 items-center justify-end gap-2">{right}</div>
        </div>
      </header>
      <main className={`mx-auto flex w-full flex-1 flex-col items-center justify-center gap-6 px-4 py-6 text-center ${width}`}>{children}</main>
      {footer && (
        <footer className="border-t-[3px] border-ink/10 bg-white/60">
          <div className={`mx-auto flex w-full flex-wrap items-center gap-3 px-4 py-3 ${width}`}>{footer}</div>
        </footer>
      )}
    </div>
  );
}

export function Logo() {
  const { t } = useTranslation();
  return (
    <span className="flex items-center gap-2 text-lg font-black tracking-tight">
      <span aria-hidden className="flex h-8 w-8 items-center justify-center rounded-xl border-[3px] border-ink bg-sun text-base leading-none shadow-[0_2px_0_var(--color-ink)]">
        ?
      </span>
      {t("app.name")}
    </span>
  );
}

/** A small label in the status bar: the PIN, the player count, the score. */
export function Chip({ children, label }: { children: ReactNode; label?: string }) {
  return (
    <span aria-label={label} className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-full border-2 border-ink bg-white px-3 py-1 text-sm font-bold tabular-nums">
      {children}
    </span>
  );
}

export function Notice({ kind, children }: { kind: "warn" | "error"; children: ReactNode }) {
  return (
    <p
      role={kind === "error" ? "alert" : "status"}
      className={`rounded-xl px-4 py-2 text-sm font-medium ${kind === "error" ? "border-2 border-coral bg-blush/40 text-ink" : "border-2 border-sun bg-sand/70 text-ink"}`}
    >
      {children}
    </p>
  );
}

export function Big({ children }: { children: ReactNode }) {
  return <p className="text-3xl font-extrabold tracking-tight text-balance sm:text-4xl">{children}</p>;
}

export function Muted({ children }: { children: ReactNode }) {
  return <p className="text-lg font-semibold text-ink/60">{children}</p>;
}

/** Three dots that pulse while everyone waits. */
export function Waiting() {
  return (
    <span aria-hidden className="flex gap-1.5">
      {[0, 1, 2].map((i) => (
        <span key={i} className="h-3 w-3 animate-bounce rounded-full border-2 border-ink bg-lagoon" style={{ animationDelay: `${i * 200}ms` }} />
      ))}
    </span>
  );
}

/** A round badge with an icon, for the "answer sent" and result screens. */
export function Badge({ children }: { children: ReactNode }) {
  return (
    <span aria-hidden className="flex h-24 w-24 animate-pop items-center justify-center rounded-full border-[3px] border-ink bg-white text-5xl font-black shadow-[0_4px_0_var(--color-ink)]">
      {children}
    </span>
  );
}

const RING = 2 * Math.PI * 44;

/** The time left, as a number inside a ring that empties. */
export function Countdown({ seconds, fraction, large }: { seconds: number; fraction: number; large?: boolean }) {
  const urgent = seconds <= 5;
  const f = Math.min(1, Math.max(0, fraction));
  return (
    <div role="timer" aria-live="off" className={`relative shrink-0 ${large ? "h-28 w-28 sm:h-36 sm:w-36" : "h-16 w-16"}`}>
      <svg viewBox="0 0 100 100" className="h-full w-full -rotate-90" aria-hidden>
        <circle cx="50" cy="50" r="44" fill="white" strokeWidth="8" className="stroke-mint" />
        <circle
          cx="50"
          cy="50"
          r="44"
          fill="none"
          strokeWidth="8"
          strokeLinecap="round"
          strokeDasharray={RING}
          strokeDashoffset={RING * (1 - f)}
          className={`transition-[stroke-dashoffset] duration-100 ease-linear ${urgent ? "stroke-coral" : "stroke-sea"}`}
        />
      </svg>
      <span className={`absolute inset-0 flex items-center justify-center font-extrabold tabular-nums ${large ? "text-4xl sm:text-5xl" : "text-2xl"} ${urgent ? "text-coral" : ""}`}>
        {seconds}
      </span>
    </div>
  );
}

/** How much of the question's time is left, from 1 down to 0. */
export function timeFraction(q: { startsAt: number; endsAt: number }, now: number): number {
  const total = q.endsAt - q.startsAt;
  return total > 0 ? (q.endsAt - now) / total : 0;
}

/** The leaderboard: ranked rows, with the player's own row picked out. */
export function Scores({ top, from = 0, me }: { top: ScoreRow[]; from?: number; me?: string }) {
  const { t } = useTranslation();
  const rows = top.slice(from);
  if (rows.length === 0) return null;
  return (
    <ol start={from + 1} className="flex w-full max-w-xl flex-col gap-2">
      {rows.map((r, j) => {
        const i = from + j;
        const mine = me !== undefined && r.nickname === me;
        return (
          <li
            key={`${r.nickname}-${i}`}
            style={{ animationDelay: `${j * 60}ms` }}
            className={`flex animate-rise items-center gap-3 rounded-2xl border-2 px-4 py-2.5 text-lg ${mine ? "border-ink bg-sand shadow-[0_3px_0_var(--color-ink)]" : "border-ink/20 bg-white/80"}`}
          >
            <span aria-hidden className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-sm font-bold tabular-nums ${i === 0 ? "bg-sun" : i === 1 ? "bg-mint" : i === 2 ? "bg-wood" : "bg-ink/5"}`}>
              {i + 1}
            </span>
            <Avatar id={r.avatar} size={36} />
            <span className="min-w-0 flex-1 truncate text-left font-bold">{r.nickname}</span>
            <span className="shrink-0 font-bold tabular-nums text-ink/70">{t("game.points", { count: r.score })}</span>
          </li>
        );
      })}
    </ol>
  );
}

const STEPS = [
  { h: "h-36 sm:h-44", block: "bg-sun", order: "order-2" },
  { h: "h-24 sm:h-32", block: "bg-lagoon", order: "order-1" },
  { h: "h-16 sm:h-24", block: "bg-wood", order: "order-3" },
];

/** The final ranking: the top three on steps, everyone else listed below. */
export function Podium({ top, me }: { top: ScoreRow[]; me?: string }) {
  const { t } = useTranslation();
  return (
    <div className="flex w-full flex-col items-center gap-6">
      <ol className="flex w-full max-w-2xl items-end justify-center gap-3 sm:gap-5">
        {top.slice(0, 3).map((r, i) => (
          <li key={`${r.nickname}-${i}`} className={`flex min-w-0 max-w-56 flex-1 flex-col items-center gap-2 ${STEPS[i].order}`}>
            <span style={{ animationDelay: `${(2 - i) * 250 + 500}ms` }} className="animate-pop">
              <Avatar id={r.avatar} size={i === 0 ? 96 : 72} />
            </span>
            <span className={`w-full truncate font-bold ${i === 0 ? "text-2xl sm:text-3xl" : "text-lg sm:text-xl"}`}>{r.nickname}</span>
            <span className="text-sm font-bold tabular-nums text-ink/70 sm:text-base">{t("game.points", { count: r.score })}</span>
            <span
              aria-hidden
              style={{ animationDelay: `${(2 - i) * 250}ms` }}
              className={`flex w-full origin-bottom animate-grow items-start justify-center rounded-t-2xl border-[3px] border-b-0 border-ink pt-3 text-3xl font-black text-ink sm:text-4xl ${STEPS[i].h} ${STEPS[i].block}`}
            >
              {i + 1}
            </span>
          </li>
        ))}
      </ol>
      <Scores top={top} from={3} me={me} />
    </div>
  );
}
