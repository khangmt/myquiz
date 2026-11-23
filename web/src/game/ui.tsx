import type { ReactNode } from "react";

// Each option has a colour and a shape, so colour-blind players can still
// tell them apart (NFR-12).
export const OPTION_STYLES = [
  { bg: "bg-red-600", shape: "▲" },
  { bg: "bg-blue-600", shape: "◆" },
  { bg: "bg-amber-500", shape: "●" },
  { bg: "bg-green-600", shape: "■" },
  { bg: "bg-purple-600", shape: "★" },
  { bg: "bg-teal-600", shape: "⬟" },
];

export function optionStyle(i: number) {
  return OPTION_STYLES[i % OPTION_STYLES.length];
}

export function Screen({ children }: { children: ReactNode }) {
  return <div className="mx-auto flex max-w-4xl flex-col items-center gap-6 py-4 text-center">{children}</div>;
}

export function Big({ children }: { children: ReactNode }) {
  return <p className="text-3xl font-bold sm:text-4xl">{children}</p>;
}

export function Countdown({ seconds }: { seconds: number }) {
  return (
    <div
      role="timer"
      aria-live="off"
      className="flex h-16 w-16 shrink-0 items-center justify-center rounded-full bg-indigo-600 text-2xl font-bold text-white"
    >
      {seconds}
    </div>
  );
}
