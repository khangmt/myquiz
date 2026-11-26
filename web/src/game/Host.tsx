import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
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
import { JoinQr } from "./JoinQr";
import { Big, btn, Chip, Countdown, Muted, Notice, optionStyle, panel, Podium, Scores, Shape, Stage, timeFraction, Waiting } from "./ui";
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
  const [confirmEnd, setConfirmEnd] = useState(false);

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
      case "reveal":
        if (s.question) lastQuestion.current = s.question;
        return s.reveal
          ? setView({ kind: "reveal", q: s.question, reveal: s.reveal })
          : setView({ kind: "between" });
      case "leaderboard":
        return s.top ? setView({ kind: "leaderboard", top: s.top }) : setView({ kind: "between" });
      case "podium":
        return s.top ? setView({ kind: "podium", top: s.top }) : setView({ kind: "between" });
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
  const live = view.kind !== "ended" && view.kind !== "error" && view.kind !== "connecting";
  const started = view.kind === "question" && now >= view.q.startsAt;
  const current = view.kind === "question" ? view.q : view.kind === "reveal" ? view.q : null;

  const nextButton = (label: string) => (
    <button type="button" className={btn.primary} disabled={busy} onClick={next}>
      {label}
    </button>
  );

  return (
    <Stage
      center={current && t("game.questionOf", { n: current.index + 1, total: current.total })}
      right={
        live &&
          view.kind !== "lobby" && (
            <>
              <Chip>
                <span className="text-white/60">PIN</span>
                <span className="tracking-wider">{pin}</span>
              </Chip>
              <Chip>
                <PersonIcon />
                {players.length}
              </Chip>
            </>
          )
      }
      footer={
        live && (
          <>
            {confirmEnd ? (
              <div className="flex items-center gap-2">
                <span className="text-sm text-white/80">{t("game.confirmEnd")}</span>
                <button
                  type="button"
                  className={btn.danger}
                  onClick={() => {
                    setConfirmEnd(false);
                    void act({ t: "host:end", d: {} });
                  }}
                >
                  {t("game.end")}
                </button>
                <button type="button" className={btn.ghost} onClick={() => setConfirmEnd(false)}>
                  {t("game.cancel")}
                </button>
              </div>
            ) : (
              // On the podium "Finish" already ends the game.
              view.kind !== "podium" && (
                <button type="button" className={btn.ghost} onClick={() => setConfirmEnd(true)}>
                  {t("game.end")}
                </button>
              )
            )}
            <div className="ml-auto flex flex-wrap items-center justify-end gap-3">
              {view.kind === "lobby" && (
                <>
                  <button type="button" className={btn.secondary} aria-pressed={locked} onClick={() => act({ t: "host:lock", d: { locked: !locked } })}>
                    {locked ? t("game.unlock") : t("game.lock")}
                  </button>
                  <button type="button" className={`${btn.primary} min-w-40`} disabled={busy || players.length === 0} onClick={() => act({ t: "host:start", d: {} })}>
                    {t("game.start")}
                  </button>
                </>
              )}
              {view.kind === "question" && started && (
                <button type="button" className={btn.secondary} onClick={() => act({ t: "host:skip", d: { index: view.q.index } })}>
                  {t("game.skip")}
                </button>
              )}
              {(view.kind === "reveal" || view.kind === "leaderboard" || view.kind === "between") && nextButton(t("game.next"))}
              {view.kind === "podium" && nextButton(t("game.finish"))}
            </div>
          </>
        )
      }
    >
      {status === "reconnecting" && <Notice kind="warn">{t("game.reconnecting")}</Notice>}
      {actionError && <Notice kind="error">{t(`game.errors.${actionError}`, { defaultValue: t("errors.INTERNAL") })}</Notice>}

      {view.kind === "connecting" && <Muted>{t("common.loading")}</Muted>}
      {view.kind === "between" && <Waiting />}

      {view.kind === "error" && (
        <>
          <Big>{t(`game.errors.${view.code}`, { defaultValue: t("errors.INTERNAL") })}</Big>
          <Link to="/quizzes" className={btn.secondary}>
            {t("nav.myQuizzes")}
          </Link>
        </>
      )}

      {view.kind === "lobby" && (
        <div className="grid w-full items-stretch gap-5 lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
          <section className={`${panel} flex flex-col items-center justify-center gap-5 p-6 sm:p-8`}>
            <p className="text-lg text-white/70">{t("game.joinAt", { url: `${joinBase.replace(/^https?:\/\//, "")}/join` })}</p>
            <p className="font-mono text-6xl font-black tracking-[0.15em] sm:text-7xl xl:text-8xl" aria-label={t("game.pinIs", { pin })}>
              {pin}
            </p>
            <JoinQr url={joinUrl} label={t("game.qr")} />
            <p className="break-all text-xs text-white/40">{joinUrl}</p>
          </section>
          <section className={`${panel} flex min-h-64 flex-col gap-4 p-6 sm:p-8`}>
            <div className="flex items-center justify-between gap-3">
              <h2 className="flex items-center gap-2 text-2xl font-bold">
                <PersonIcon />
                {t("game.playerCount", { count: players.length })}
              </h2>
              {locked && <Chip>{t("game.lockedBadge")}</Chip>}
            </div>
            {players.length === 0 ? (
              <div className="flex flex-1 flex-col items-center justify-center gap-4">
                <Waiting />
                <Muted>{t("game.waitingForPlayers")}</Muted>
              </div>
            ) : (
              <ul className="flex flex-wrap content-start gap-2">
                {players.map((p) => (
                  <li key={p.id} className="group flex animate-pop items-center rounded-full bg-white/10 py-1 pl-4 pr-1 text-lg font-semibold">
                    {p.nickname}
                    <button
                      type="button"
                      aria-label={t("game.kick", { name: p.nickname })}
                      className="ml-1 flex h-7 w-7 items-center justify-center rounded-full text-sm text-white/40 transition hover:bg-rose-600 hover:text-white focus-visible:bg-rose-600 focus-visible:text-white"
                      onClick={() => act({ t: "host:kick", d: { playerId: p.id } })}
                    >
                      ✕
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}

      {view.kind === "question" &&
        (!started ? (
          <>
            <Muted>{t("game.getReady", { s: secondsLeft(view.q.startsAt, now) })}</Muted>
            <QuestionText>{view.q.text}</QuestionText>
          </>
        ) : (
          <div className="flex w-full flex-1 flex-col gap-5">
            <QuestionText>{view.q.text}</QuestionText>
            <div className={`grid grid-cols-[1fr_auto_1fr] items-center gap-4 sm:gap-8 ${view.q.media ? "flex-1" : ""}`}>
              <div className="flex justify-start">
                <Countdown large seconds={secondsLeft(view.q.endsAt, now)} fraction={timeFraction(view.q, now)} />
              </div>
              {view.q.media ? <img src={view.q.media} alt="" className="max-h-[38vh] min-w-0 rounded-2xl object-contain shadow-2xl shadow-black/40" /> : <span />}
              <p className="justify-self-end rounded-2xl bg-white/10 px-4 py-3 text-xl font-bold tabular-nums sm:text-2xl" aria-live="polite">
                {t("game.answered", { answered: view.answered, connected: view.connected })}
              </p>
            </div>
            <OptionGrid q={view.q} fill={!view.q.media} />
          </div>
        ))}

      {view.kind === "reveal" && (
        <div className="flex w-full flex-1 flex-col justify-center gap-6">
          {view.q && <QuestionText>{view.q.text}</QuestionText>}
          <RevealView q={view.q} reveal={view.reveal} />
        </div>
      )}

      {view.kind === "leaderboard" && (
        <>
          <Big>{t("game.leaderboard")}</Big>
          <Scores top={view.top} />
        </>
      )}

      {view.kind === "podium" && (
        <>
          <Big>{t("game.podium")}</Big>
          <Podium top={view.top} />
        </>
      )}

      {view.kind === "ended" && (
        <div className={`${panel} flex w-full max-w-xl flex-col items-center gap-6 p-8`}>
          <Big>{t(`game.ended.${view.reason}`, { defaultValue: t("game.ended.finished") })}</Big>
          <div className="flex flex-wrap items-center justify-center gap-3">
            {reportId && (
              <Link to={`/reports/${reportId}`} className={btn.primary}>
                {t("reports.view")}
              </Link>
            )}
            <Link to="/quizzes" className={btn.secondary}>
              {t("nav.myQuizzes")}
            </Link>
          </div>
        </div>
      )}
    </Stage>
  );
}

function PersonIcon() {
  return (
    <svg aria-hidden viewBox="0 0 20 20" fill="currentColor" className="h-[1em] w-[1em] opacity-70">
      <path d="M10 10a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm-7 8a7 7 0 0 1 14 0 1 1 0 0 1-1 1H4a1 1 0 0 1-1-1Z" />
    </svg>
  );
}

/** The question, as large as the room needs. */
function QuestionText({ children }: { children: ReactNode }) {
  return (
    <h1 className={`${panel} w-full px-6 py-5 text-3xl font-extrabold tracking-tight text-balance sm:py-7 sm:text-4xl lg:text-5xl`}>{children}</h1>
  );
}

/**
 * The options as tiles. With `counts`, each tile also shows how many players
 * chose it, and the wrong ones step back.
 */
function OptionGrid({ q, correct, counts, fill }: { q: QuestionStart; correct?: number[]; counts?: Map<number, number>; fill?: boolean }) {
  const { t } = useTranslation();
  if (q.type === "fill_blank") return null;
  const max = counts ? Math.max(1, ...counts.values()) : 1;
  return (
    <ul className={`grid w-full grid-cols-1 gap-3 sm:grid-cols-2 ${fill ? "flex-1 auto-rows-fr" : ""}`} aria-label={counts ? t("game.distribution") : undefined}>
      {q.options.map((o, i) => {
        const right = correct?.includes(o.id);
        const n = counts?.get(o.id) ?? 0;
        return (
          <li
            key={o.id}
            className={`flex flex-col justify-center gap-3 rounded-2xl p-4 text-left text-xl font-bold shadow-lg shadow-black/20 transition sm:p-5 sm:text-2xl ${optionStyle(i).bg} ${
              correct ? (right ? "ring-4 ring-white" : "opacity-50 saturate-50") : ""
            }`}
          >
            <div className="flex items-center gap-3">
              <Shape i={i} />
              <span className="min-w-0 flex-1 break-words">{o.text}</span>
              {right && (
                <span aria-hidden className="flex h-9 w-9 shrink-0 animate-pop items-center justify-center rounded-full bg-white text-xl text-emerald-700">
                  ✓
                </span>
              )}
              {counts && <span className="shrink-0 text-3xl font-black tabular-nums">{n}</span>}
            </div>
            {counts && (
              <div aria-hidden className="h-2 w-full overflow-hidden rounded-full bg-black/25">
                <div className="h-full origin-left rounded-full bg-white transition-[width] duration-700" style={{ width: `${(n / max) * 100}%` }} />
              </div>
            )}
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
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-4">
        <p className="animate-pop rounded-2xl bg-emerald-600 px-6 py-5 text-2xl font-bold ring-4 ring-white sm:text-3xl">
          {t("game.acceptedAnswers", { answers: reveal.acceptedAnswers.join(", ") })}
        </p>
        <ul className="flex flex-col gap-2">
          {reveal.topTextAnswers.map((a) => (
            <li key={a.text} className={`flex items-center gap-3 rounded-xl border px-4 py-2 text-lg ${a.correct ? "border-emerald-400/50 bg-emerald-500/20 font-semibold" : "border-white/10 bg-white/[0.07] text-white/70"}`}>
              <span aria-hidden className="w-5">
                {a.correct ? "✓" : ""}
              </span>
              <span className="min-w-0 flex-1 truncate text-left">{a.text}</span>
              <span className="font-bold tabular-nums">{a.count}</span>
            </li>
          ))}
        </ul>
      </div>
    );
  }
  if (!q) return null;
  const counts = new Map(q.options.map((o) => [o.id, 0]));
  for (const d of reveal.distribution) counts.set(d.optionId, d.count);
  return <OptionGrid q={q} correct={reveal.correctOptionIds} counts={counts} />;
}
