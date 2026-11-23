import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { ClientConfig } from "../generated/ClientConfig";
import { useTranslation } from "react-i18next";
import type { GameSnapshot } from "../generated/GameSnapshot";
import type { GameState } from "../generated/GameState";
import type { LobbyPlayer } from "../generated/LobbyPlayer";
import type { QuestionReveal } from "../generated/QuestionReveal";
import type { QuestionStart } from "../generated/QuestionStart";
import type { ScoreRow } from "../generated/ScoreRow";
import type { ServerPush } from "../generated/ServerPush";
import { AckError, GameSocket, type SocketStatus } from "./socket";
import { Big, Countdown, optionStyle, Screen } from "./ui";
import { secondsLeft, useTicker } from "./useCountdown";

type View =
  | { kind: "connecting" }
  | { kind: "lobby" }
  | { kind: "question"; q: QuestionStart; answered: number; connected: number }
  | { kind: "reveal"; q: QuestionStart | null; reveal: QuestionReveal }
  | { kind: "leaderboard"; top: ScoreRow[] }
  | { kind: "podium"; top: ScoreRow[] }
  | { kind: "between" }
  | { kind: "ended"; reason: string }
  | { kind: "error"; code: string };

/** The host's screen, usually on a projector (FR-GM-01 to 12). */
export function Host() {
  const { pin = "" } = useParams();
  const { t } = useTranslation();
  const [view, setView] = useState<View>({ kind: "connecting" });
  const [players, setPlayers] = useState<LobbyPlayer[]>([]);
  const [locked, setLocked] = useState(false);
  const [status, setStatus] = useState<SocketStatus>("connecting");
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const state = useRef<GameState>({ state: "lobby" });
  const lastQuestion = useRef<QuestionStart | null>(null);
  const socket = useRef<GameSocket | null>(null);
  const [reportId, setReportId] = useState<string | null>(null);

  const fromSnapshot = useCallback((s: GameSnapshot) => {
    state.current = s.state;
    if (s.reportId) setReportId(s.reportId);
    setPlayers(s.players);
    setLocked(s.locked);
    switch (s.state.state) {
      case "lobby":
        return setView({ kind: "lobby" });
      case "question":
        lastQuestion.current = s.question;
        return s.question
          ? setView({ kind: "question", q: s.question, answered: 0, connected: s.players.length })
          : setView({ kind: "between" });
      case "ended":
        return setView({ kind: "ended", reason: "finished" });
      default:
        return setView({ kind: "between" });
    }
  }, []);

  const onPush = useCallback((p: ServerPush) => {
    switch (p.t) {
      case "lobby:update":
        setPlayers(p.d.players);
        setLocked(p.d.locked);
        return;
      case "question:start":
        state.current = { state: "question", index: p.d.index };
        lastQuestion.current = p.d;
        return setView({ kind: "question", q: p.d, answered: 0, connected: 0 });
      case "question:progress":
        return setView((v) => (v.kind === "question" ? { ...v, answered: p.d.answered, connected: p.d.connected } : v));
      case "question:reveal":
        state.current = { state: "reveal", index: p.d.index };
        return setView({ kind: "reveal", q: lastQuestion.current, reveal: p.d });
      case "leaderboard:show": {
        const s = state.current;
        const index = "index" in s ? s.index : 0;
        state.current = { state: "leaderboard", index };
        return setView({ kind: "leaderboard", top: p.d.top });
      }
      case "game:podium":
        state.current = { state: "podium" };
        return setView({ kind: "podium", top: p.d.top });
      case "game:ended":
        state.current = { state: "ended" };
        return setView({ kind: "ended", reason: p.d.reason });
      default:
        return;
    }
  }, []);

  useEffect(() => {
    const s = new GameSocket({
      onPush,
      onStatus: setStatus,
      onOpen: () => {
        s.request<GameSnapshot>({ t: "host:attach", d: { pin } })
          .then(fromSnapshot)
          .catch((e) => setView({ kind: "error", code: e instanceof AckError ? e.code : "INTERNAL" }));
      },
    });
    socket.current = s;
    return () => s.close();
  }, [pin, onPush, fromSnapshot]);

  const now = useTicker(view.kind === "question", useCallback(() => socket.current?.serverNow() ?? Date.now(), []));

  async function act(msg: Parameters<GameSocket["request"]>[0]) {
    if (!socket.current) return;
    setBusy(true);
    setActionError(null);
    try {
      await socket.current.request(msg);
    } catch (e) {
      const code = e instanceof AckError ? e.code : "INTERNAL";
      // STALE means the game already moved on (a double click): harmless.
      if (code !== "STALE") setActionError(code);
    } finally {
      setBusy(false);
    }
  }
  const next = () => act({ t: "host:next", d: { expect: state.current } });

  // The server knows the address phones should use (JOIN_URL, else
  // PUBLIC_URL); the browser's own origin may be localhost.
  const config = useQuery({ queryKey: ["config"], queryFn: () => api<ClientConfig>("/config"), staleTime: Infinity });
  const joinBase = (config.data?.joinUrl ?? window.location.origin).replace(/\/$/, "");
  const joinUrl = `${joinBase}/join?pin=${pin}`;
  const button = "rounded-lg bg-indigo-600 px-5 py-3 text-lg font-semibold text-white disabled:opacity-50";

  return (
    <Screen>
      {status === "reconnecting" && (
        <p role="status" className="rounded bg-amber-100 px-3 py-1 text-amber-900">
          {t("game.reconnecting")}
        </p>
      )}
      {actionError && (
        <p role="alert" className="rounded bg-red-50 px-3 py-1 text-red-800">
          {t(`game.errors.${actionError}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}

      {view.kind === "connecting" && <p className="text-slate-500">{t("common.loading")}</p>}

      {view.kind === "error" && (
        <>
          <Big>{t(`game.errors.${view.code}`, { defaultValue: t("errors.INTERNAL") })}</Big>
          <Link to="/quizzes" className="underline">
            {t("nav.myQuizzes")}
          </Link>
        </>
      )}

      {view.kind === "lobby" && (
        <>
          <p className="text-lg text-slate-600">{t("game.joinAt", { url: `${joinBase.replace(/^https?:\/\//, "")}/join` })}</p>
          <p className="font-mono text-6xl font-bold tracking-widest sm:text-7xl" aria-label={t("game.pinIs", { pin })}>
            {pin}
          </p>
          <p className="break-all text-sm text-slate-500">{joinUrl}</p>
          <p className="text-xl font-semibold">{t("game.playerCount", { count: players.length })}</p>
          <ul className="flex flex-wrap justify-center gap-2">
            {players.map((p) => (
              <li key={p.id} className="flex items-center gap-1 rounded-full bg-white px-3 py-1 shadow">
                {p.nickname}
                <button
                  type="button"
                  aria-label={t("game.kick", { name: p.nickname })}
                  className="ml-1 text-sm text-red-700"
                  onClick={() => act({ t: "host:kick", d: { playerId: p.id } })}
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <div className="flex flex-wrap justify-center gap-3">
            <button type="button" className="rounded-lg border border-slate-300 bg-white px-4 py-3" onClick={() => act({ t: "host:lock", d: { locked: !locked } })}>
              {locked ? t("game.unlock") : t("game.lock")}
            </button>
            <button type="button" className={button} disabled={busy || players.length === 0} onClick={() => act({ t: "host:start", d: {} })}>
              {t("game.start")}
            </button>
          </div>
        </>
      )}

      {view.kind === "question" && (
        <>
          <p className="text-slate-600">{t("game.questionOf", { n: view.q.index + 1, total: view.q.total })}</p>
          {now < view.q.startsAt ? (
            <>
              <Big>{view.q.text}</Big>
              <p className="text-2xl">{t("game.getReady", { s: secondsLeft(view.q.startsAt, now) })}</p>
            </>
          ) : (
            <>
              <div className="flex w-full items-center justify-between gap-4">
                <Countdown seconds={secondsLeft(view.q.endsAt, now)} />
                <Big>{view.q.text}</Big>
                <p className="text-lg" aria-live="polite">
                  {t("game.answered", { answered: view.answered, connected: view.connected })}
                </p>
              </div>
              {view.q.media && <img src={view.q.media} alt="" className="max-h-72 rounded" />}
              <OptionGrid q={view.q} />
              <button type="button" className="rounded-lg border border-slate-300 bg-white px-4 py-2" onClick={() => act({ t: "host:skip", d: { index: view.q.index } })}>
                {t("game.skip")}
              </button>
            </>
          )}
        </>
      )}

      {view.kind === "reveal" && (
        <>
          {view.q && <Big>{view.q.text}</Big>}
          <RevealView q={view.q} reveal={view.reveal} />
          <button type="button" className={button} disabled={busy} onClick={next}>
            {t("game.next")}
          </button>
        </>
      )}

      {view.kind === "leaderboard" && (
        <>
          <Big>{t("game.leaderboard")}</Big>
          <Scores top={view.top} />
          <button type="button" className={button} disabled={busy} onClick={next}>
            {t("game.next")}
          </button>
        </>
      )}

      {view.kind === "between" && (
        <button type="button" className={button} disabled={busy} onClick={next}>
          {t("game.next")}
        </button>
      )}

      {view.kind === "podium" && (
        <>
          <Big>{t("game.podium")}</Big>
          <Scores top={view.top} podium />
          <button type="button" className={button} disabled={busy} onClick={next}>
            {t("game.finish")}
          </button>
        </>
      )}

      {view.kind === "ended" && (
        <>
          <Big>{t(`game.ended.${view.reason}`, { defaultValue: t("game.ended.finished") })}</Big>
          {reportId && (
            <Link to={`/reports/${reportId}`} className={button}>
              {t("reports.view")}
            </Link>
          )}
          <Link to="/quizzes" className="underline">
            {t("nav.myQuizzes")}
          </Link>
        </>
      )}

      {view.kind !== "ended" && view.kind !== "error" && view.kind !== "connecting" && (
        <button
          type="button"
          className="mt-6 text-sm text-red-700 underline"
          onClick={() => {
            if (window.confirm(t("game.confirmEnd"))) void act({ t: "host:end", d: {} });
          }}
        >
          {t("game.end")}
        </button>
      )}
    </Screen>
  );
}

function OptionGrid({ q, correct }: { q: QuestionStart; correct?: number[] }) {
  if (q.type === "fill_blank") return null;
  return (
    <ul className="grid w-full grid-cols-1 gap-3 sm:grid-cols-2">
      {q.options.map((o, i) => {
        const s = optionStyle(i);
        const dim = correct && !correct.includes(o.id);
        return (
          <li key={o.id} className={`flex items-center gap-3 rounded-lg p-4 text-left text-xl font-semibold text-white ${s.bg} ${dim ? "opacity-40" : ""}`}>
            <span aria-hidden>{s.shape}</span>
            {o.text}
            {correct?.includes(o.id) && <span className="ml-auto">✓</span>}
          </li>
        );
      })}
    </ul>
  );
}

function RevealView({ q, reveal }: { q: QuestionStart | null; reveal: QuestionReveal }) {
  const { t } = useTranslation();
  if (reveal.acceptedAnswers.length > 0 || q?.type === "fill_blank") {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-2xl">{t("game.acceptedAnswers", { answers: reveal.acceptedAnswers.join(", ") })}</p>
        <ul className="flex flex-col gap-1">
          {reveal.topTextAnswers.map((a) => (
            <li key={a.text} className={a.correct ? "font-semibold text-green-700" : "text-slate-600"}>
              {a.text} · {a.count}
            </li>
          ))}
        </ul>
      </div>
    );
  }
  const counts = new Map(reveal.distribution.map((d) => [d.optionId, d.count]));
  const max = Math.max(1, ...reveal.distribution.map((d) => d.count));
  return (
    <div className="flex w-full flex-col gap-4">
      <div className="flex h-40 items-end justify-center gap-4" aria-label={t("game.distribution")}>
        {q?.options.map((o, i) => {
          const n = counts.get(o.id) ?? 0;
          return (
            <div key={o.id} className="flex w-16 flex-col items-center gap-1">
              <span className="font-semibold">{n}</span>
              <div className={`w-full rounded-t ${optionStyle(i).bg}`} style={{ height: `${(n / max) * 120 + 4}px` }} />
              <span aria-hidden>{optionStyle(i).shape}</span>
            </div>
          );
        })}
      </div>
      {q && <OptionGrid q={q} correct={reveal.correctOptionIds} />}
    </div>
  );
}

export function Scores({ top, podium }: { top: ScoreRow[]; podium?: boolean }) {
  const { t } = useTranslation();
  return (
    <ol className="flex w-full max-w-md flex-col gap-2">
      {top.map((r, i) => (
        <li key={`${r.nickname}-${i}`} className={`flex justify-between rounded-lg bg-white px-4 py-3 shadow ${podium && i === 0 ? "text-2xl font-bold" : "text-lg"}`}>
          <span>
            {podium ? ["🥇", "🥈", "🥉"][i] ?? `${i + 1}.` : `${i + 1}.`} {r.nickname}
          </span>
          <span>{t("game.points", { count: r.score })}</span>
        </li>
      ))}
    </ol>
  );
}
