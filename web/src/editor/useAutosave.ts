import { useCallback, useEffect, useRef, useState } from "react";
import { api, ApiFailure } from "../api/client";
import type { QuizDoc } from "../generated/QuizDoc";
import type { QuizProblemDoc } from "../generated/QuizProblemDoc";
import { toSave, type Draft } from "./model";

export type SaveState =
  | { kind: "saved" }
  | { kind: "pending" }
  | { kind: "saving" }
  | { kind: "conflict" }
  | { kind: "invalid"; problems: QuizProblemDoc[] }
  | { kind: "error"; code: string };

/** Saves the whole quiz 1.5 s after the last change (FR-QZ-08: within 2 s). */
export const AUTOSAVE_MS = 1500;

/**
 * Autosave with optimistic versioning: every PUT sends If-Match with the
 * version we last saw. Another tab saving first gives `conflict`, and the
 * editor stops saving until the page is reloaded, so nothing is lost
 * silently.
 */
export function useAutosave(id: string, initialVersion: number, onSaved: (doc: QuizDoc) => void) {
  const version = useRef(initialVersion);
  const latest = useRef<Draft | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const inFlight = useRef<Promise<void> | null>(null);
  const [state, setState] = useState<SaveState>({ kind: "saved" });
  const stopped = useRef(false);
  const onSavedRef = useRef(onSaved);
  onSavedRef.current = onSaved;

  const flush = useCallback(async (): Promise<boolean> => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    if (inFlight.current) await inFlight.current;
    const draft = latest.current;
    if (!draft || stopped.current) return !stopped.current;
    latest.current = null;
    setState({ kind: "saving" });
    let ok = false;
    const run = (async () => {
      try {
        const doc = await api<QuizDoc>(`/quizzes/${id}`, {
          method: "PUT",
          json: toSave(draft),
          headers: { "If-Match": `"${version.current}"` },
        });
        version.current = doc.version;
        ok = true;
        setState(latest.current ? { kind: "pending" } : { kind: "saved" });
        onSavedRef.current(doc);
      } catch (e) {
        if (e instanceof ApiFailure && e.code === "VERSION_CONFLICT") {
          stopped.current = true;
          setState({ kind: "conflict" });
        } else if (e instanceof ApiFailure && e.code === "QUIZ_INVALID") {
          const problems = (e.body as { problems?: QuizProblemDoc[] }).problems ?? [];
          setState({ kind: "invalid", problems });
        } else {
          // Keep the edit so the next change (or Retry) sends it again.
          latest.current = latest.current ?? draft;
          setState({ kind: "error", code: e instanceof ApiFailure ? e.code : "INTERNAL" });
        }
      }
    })();
    inFlight.current = run;
    await run;
    inFlight.current = null;
    return ok;
  }, [id]);

  const change = useCallback(
    (draft: Draft) => {
      if (stopped.current) return;
      latest.current = draft;
      setState({ kind: "pending" });
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => void flush(), AUTOSAVE_MS);
    },
    [flush],
  );

  // Warn before leaving with unsaved changes; save when the tab is hidden.
  useEffect(() => {
    const beforeUnload = (e: BeforeUnloadEvent) => {
      if (latest.current || inFlight.current) e.preventDefault();
    };
    const hidden = () => {
      if (document.visibilityState === "hidden" && latest.current) void flush();
    };
    window.addEventListener("beforeunload", beforeUnload);
    document.addEventListener("visibilitychange", hidden);
    return () => {
      window.removeEventListener("beforeunload", beforeUnload);
      document.removeEventListener("visibilitychange", hidden);
      // Leaving the editor inside the app: send what is pending.
      if (latest.current) void flush();
    };
  }, [flush]);

  return { state, change, flush };
}
