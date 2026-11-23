import { useEffect, useState } from "react";

/** Re-renders every 100 ms while `active`, returning the current time. */
export function useTicker(active: boolean, now: () => number): number {
  const [t, setT] = useState(now);
  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => setT(now()), 100);
    return () => clearInterval(id);
  }, [active, now]);
  return t;
}

/** Whole seconds left until `endsAt`, never negative. */
export function secondsLeft(endsAt: number, now: number): number {
  return Math.max(0, Math.ceil((endsAt - now) / 1000));
}
