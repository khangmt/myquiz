import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { api, ApiFailure } from "../api/client";
import type { GameSnapshot } from "../generated/GameSnapshot";
import type { PinCheck } from "../generated/PinCheck";
import type { PlayerJoined } from "../generated/PlayerJoined";
import type { QuestionResult } from "../generated/QuestionResult";
import type { QuestionStart } from "../generated/QuestionStart";
import type { ScoreRow } from "../generated/ScoreRow";
import type { ServerPush } from "../generated/ServerPush";
import { Scores } from "./Host";
import { AckError, GameSocket, type SocketStatus } from "./socket";
import { Big, Countdown, optionStyle, Screen } from "./ui";
import { secondsLeft, useTicker } from "./useCountdown";

const DEVICE_KEY = "myquiz.device";
const tokenKey = (pin: string) => `myquiz.player.${pin}`;

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** One id per browser, so the server can stop one phone joining twice. */
export function deviceId(): string {
  const s = storage();
  let id = s?.getItem(DEVICE_KEY);
  if (!id) {
    id = crypto.randomUUID();
    s?.setItem(DEVICE_KEY, id);
  }
  return id;
}

type Stage =
  | { kind: "pin" }
  | { kind: "checking" }
  | { kind: "nickname"; game: PinCheck }
  | { kind: "playing" };

/** `/join?pin=`: the player's flow on a phone (FR-PL-01 to 10). */
export function Play() {
  const { t } = useTranslation();
  const [params, setParams] = useSearchParams();
  const pin = params.get("pin") ?? "";
  const [stage, setStage] = useState<Stage>(() =>
    pin && storage()?.getItem(tokenKey(pin)) ? { kind: "playing" } : { kind: pin ? "checking" : "pin" },
  );
  const [error, setError] = useState<string | null>(null);
  const [nickname, setNickname] = useState("");
  const [pinInput, setPinInput] = useState(pin);

  useEffect(() => {
    if (stage.kind !== "checking") return;
    let live = true;
    api<PinCheck>(`/games/pin/${encodeURIComponent(pin)}`)
      .then((game) => {
        if (!live) return;
        if (game.locked) {
          setError("LOCKED");
          setStage({ kind: "pin" });
        } else setStage({ kind: "nickname", game });
      })
      .catch((e) => {
        if (!live) return;
        setError(e instanceof ApiFailure && e.status === 404 ? "PIN_INVALID" : e instanceof ApiFailure ? e.code : "INTERNAL");
        setStage({ kind: "pin" });
      });
    return () => {
      live = false;
    };
  }, [stage.kind, pin]);

  if (stage.kind === "playing") {
    return (
      <Game
        pin={pin}
        nickname={nickname}
        onLeave={(code) => {
          storage()?.removeItem(tokenKey(pin));
          setError(code);
          setStage({ kind: code === "NICK_TAKEN" || code === "NICK_INVALID" ? "checking" : "pin" });
        }}
      />
    );
  }

  const input = "w-full rounded-lg border border-slate-300 px-3 py-3 text-center text-2xl";
  const button = "w-full rounded-lg bg-indigo-600 px-4 py-3 text-xl font-semibold text-white disabled:opacity-50";
  return (
    <Screen>
      {error && (
        <p role="alert" className="rounded bg-red-50 px-3 py-2 text-red-800">
          {t(`game.errors.${error}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}
      {stage.kind === "checking" && <p className="text-slate-500">{t("common.loading")}</p>}
      {stage.kind === "pin" && (
        <form
          className="flex w-full max-w-xs flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (pinInput.length !== 6) return;
            setError(null);
            setParams({ pin: pinInput });
            setStage({ kind: "checking" });
          }}
        >
          <input
            className={`${input} tracking-widest`}
            inputMode="numeric"
            autoComplete="off"
            maxLength={6}
            aria-label={t("home.pinPlaceholder")}
            placeholder={t("home.pinPlaceholder")}
            value={pinInput}
            onChange={(e) => setPinInput(e.target.value.replace(/\D/g, ""))}
          />
          <button className={button} type="submit" disabled={pinInput.length !== 6}>
            {t("home.join")}
          </button>
        </form>
      )}
      {stage.kind === "nickname" && (
        <form
          className="flex w-full max-w-xs flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (!nickname.trim()) return;
            setError(null);
            setStage({ kind: "playing" });
          }}
        >
          <p className="text-xl font-semibold">{stage.game.title}</p>
          <input
            className={input}
            autoFocus
            maxLength={20}
            aria-label={t("game.nickname")}
            placeholder={t("game.nickname")}
            value={nickname}
            onChange={(e) => setNickname(e.target.value)}
          />
          <button className={button} type="submit" disabled={!nickname.trim()}>
            {t("game.enter")}
          </button>
        </form>
      )}
    </Screen>
  );
}

type View =
  | { kind: "connecting" }
  | { kind: "lobby" }
  | { kind: "question"; q: QuestionStart; sent: boolean }
  | { kind: "result"; r: QuestionResult }
  | { kind: "waiting" }
  | { kind: "leaderboard"; top: ScoreRow[] }
  | { kind: "podium"; top: ScoreRow[] }
  | { kind: "ended"; reason: string }
  | { kind: "kicked" };

/** Leave codes send the player back to the PIN or nickname screen. */
const FATAL = new Set(["PIN_INVALID", "LOCKED", "KICKED", "FULL", "NICK_TAKEN", "NICK_INVALID", "TOKEN_INVALID", "ALREADY_JOINED"]);

function Game({ pin, nickname, onLeave }: { pin: string; nickname: string; onLeave: (code: string) => void }) {
  const { t } = useTranslation();
  const [view, setView] = useState<View>({ kind: "connecting" });
  const [me, setMe] = useState<{ nickname: string; score: number; rank: number } | null>(null);
  const [status, setStatus] = useState<SocketStatus>("connecting");
  const [submitError, setSubmitError] = useState<string | null>(null);
  const socket = useRef<GameSocket | null>(null);
  const leave = useRef(onLeave);
  leave.current = onLeave;

  const fromSnapshot = useCallback((s: GameSnapshot) => {
    if (s.me) setMe({ nickname: s.me.nickname, score: s.me.score, rank: s.me.rank });
    switch (s.state.state) {
      case "lobby":
        return setView({ kind: "lobby" });
      case "question":
        return s.question ? setView({ kind: "question", q: s.question, sent: !!s.me?.answered }) : setView({ kind: "waiting" });
      case "reveal":
        return s.result ? setView({ kind: "result", r: s.result }) : setView({ kind: "waiting" });
      case "leaderboard":
        return s.top ? setView({ kind: "leaderboard", top: s.top }) : setView({ kind: "waiting" });
      case "podium":
        return s.top ? setView({ kind: "podium", top: s.top }) : setView({ kind: "waiting" });
      case "ended":
        return setView({ kind: "ended", reason: "finished" });
      default:
        return setView({ kind: "waiting" });
    }
  }, []);

  const onPush = useCallback((p: ServerPush) => {
    switch (p.t) {
      case "question:start":
        setSubmitError(null);
        return setView({ kind: "question", q: p.d, sent: false });
      case "question:result":
        setMe((m) => (m ? { ...m, score: p.d.total, rank: p.d.rank } : m));
        return setView({ kind: "result", r: p.d });
      case "question:reveal":
        return;
      case "leaderboard:show":
        return setView({ kind: "leaderboard", top: p.d.top });
      case "game:podium":
        return setView({ kind: "podium", top: p.d.top });
      case "game:ended":
        storage()?.removeItem(tokenKey(pin));
        socket.current?.close();
        return setView({ kind: "ended", reason: p.d.reason });
      case "player:kicked":
        storage()?.removeItem(tokenKey(pin));
        socket.current?.close();
        return setView({ kind: "kicked" });
      default:
        return;
    }
  }, [pin]);

  useEffect(() => {
    const s = new GameSocket({
      onPush,
      onStatus: setStatus,
      onOpen: () => {
        const token = storage()?.getItem(tokenKey(pin));
        const attempt = token
          ? s.request<GameSnapshot>({ t: "player:resume", d: { pin, token } })
          : s.request<PlayerJoined>({ t: "player:join", d: { pin, nickname: nickname.trim(), deviceId: deviceId() } }).then((j) => {
              storage()?.setItem(tokenKey(pin), j.token);
              return j.snapshot;
            });
        attempt.then(fromSnapshot).catch((e) => {
          const code = e instanceof AckError ? e.code : "INTERNAL";
          if (FATAL.has(code)) {
            s.close();
            leave.current(code);
          }
          // Anything else (a timeout, a dropped socket) is retried on the
          // next reconnect.
        });
      },
    });
    socket.current = s;
    return () => s.close();
    // nickname is read once, at join time.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pin, onPush, fromSnapshot]);

  const now = useTicker(view.kind === "question", useCallback(() => socket.current?.serverNow() ?? Date.now(), []));

  async function submit(q: QuestionStart, optionIds: number[], text: string | null) {
    if (!socket.current) return;
    setSubmitError(null);
    setView({ kind: "question", q, sent: true });
    try {
      await socket.current.request({ t: "answer:submit", d: { index: q.index, optionIds, text } });
    } catch (e) {
      const code = e instanceof AckError ? e.code : "INTERNAL";
      if (code === "DUPLICATE") return;
      setSubmitError(code);
      // Let them try again unless the time is up.
      if (code !== "LATE" && code !== "WRONG_STATE") setView((v) => (v.kind === "question" ? { ...v, sent: false } : v));
    }
  }

  return (
    <Screen>
      {status === "reconnecting" && (
        <p role="status" className="rounded bg-amber-100 px-3 py-1 text-amber-900">
          {t("game.reconnecting")}
        </p>
      )}
      {me && view.kind !== "ended" && view.kind !== "kicked" && (
        <p className="text-sm text-slate-600">
          {me.nickname} · {t("game.points", { count: me.score })}
        </p>
      )}
      {submitError && (
        <p role="alert" className="rounded bg-red-50 px-3 py-1 text-red-800">
          {t(`game.errors.${submitError}`, { defaultValue: t("errors.INTERNAL") })}
        </p>
      )}

      {view.kind === "connecting" && <p className="text-slate-500">{t("common.loading")}</p>}
      {view.kind === "lobby" && (
        <>
          <Big>{t("game.youreIn")}</Big>
          <p className="text-slate-600">{t("game.waitForHost")}</p>
        </>
      )}
      {view.kind === "waiting" && <p className="text-xl text-slate-600">{t("game.waitForNext")}</p>}

      {view.kind === "question" &&
        (now < view.q.startsAt ? (
          <>
            <p className="text-slate-600">{t("game.questionOf", { n: view.q.index + 1, total: view.q.total })}</p>
            <Big>{view.q.text}</Big>
            <p className="text-2xl">{t("game.getReady", { s: secondsLeft(view.q.startsAt, now) })}</p>
          </>
        ) : view.sent ? (
          <Big>{t("game.answerSent")}</Big>
        ) : now >= view.q.endsAt ? (
          <Big>{t("game.timeUp")}</Big>
        ) : (
          <>
            <div className="flex w-full items-center justify-between gap-3">
              <Countdown seconds={secondsLeft(view.q.endsAt, now)} />
              <p className="text-lg font-semibold">{view.q.text}</p>
            </div>
            <AnswerPad q={view.q} onSubmit={(ids, text) => submit(view.q, ids, text)} />
          </>
        ))}

      {view.kind === "result" && (
        <div className={`flex w-full flex-col gap-2 rounded-xl p-6 text-white ${view.r.correct ? "bg-green-600" : view.r.answered ? "bg-red-600" : "bg-slate-600"}`}>
          <Big>{!view.r.answered ? t("game.timeUp") : view.r.correct ? t("game.correct") : t("game.incorrect")}</Big>
          <p className="text-2xl">+{view.r.points}</p>
          <p>{t("game.rankIs", { rank: view.r.rank })}</p>
        </div>
      )}

      {view.kind === "leaderboard" && (
        <>
          <Big>{t("game.leaderboard")}</Big>
          <Scores top={view.top} />
          {me && <p>{t("game.rankIs", { rank: me.rank })}</p>}
        </>
      )}
      {view.kind === "podium" && (
        <>
          <Big>{t("game.podium")}</Big>
          <Scores top={view.top} podium />
          {me && <p>{t("game.finalRank", { rank: me.rank, count: me.score })}</p>}
        </>
      )}
      {view.kind === "ended" && (
        <>
          <Big>{t(`game.ended.${view.reason}`, { defaultValue: t("game.ended.finished") })}</Big>
          {me && <p>{t("game.finalRank", { rank: me.rank, count: me.score })}</p>}
        </>
      )}
      {view.kind === "kicked" && <Big>{t("game.errors.KICKED")}</Big>}
    </Screen>
  );
}

function AnswerPad({ q, onSubmit }: { q: QuestionStart; onSubmit: (ids: number[], text: string | null) => void }) {
  const { t } = useTranslation();
  const [picked, setPicked] = useState<number[]>([]);
  const [text, setText] = useState("");

  if (q.type === "fill_blank") {
    return (
      <form
        className="flex w-full flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (text.trim()) onSubmit([], text.trim());
        }}
      >
        <input
          className="w-full rounded-lg border border-slate-300 px-3 py-3 text-xl"
          autoFocus
          maxLength={100}
          aria-label={t("game.yourAnswer")}
          placeholder={t("game.yourAnswer")}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <button type="submit" disabled={!text.trim()} className="rounded-lg bg-indigo-600 px-4 py-3 text-xl font-semibold text-white disabled:opacity-50">
          {t("game.submit")}
        </button>
      </form>
    );
  }

  const multi = q.type === "multi";
  return (
    <div className="flex w-full flex-col gap-3">
      <div className="grid w-full grid-cols-2 gap-3">
        {q.options.map((o, i) => {
          const s = optionStyle(i);
          const on = picked.includes(o.id);
          return (
            <button
              key={o.id}
              type="button"
              aria-pressed={multi ? on : undefined}
              className={`flex min-h-24 items-center gap-2 rounded-lg p-4 text-left text-lg font-semibold text-white ${s.bg} ${multi && on ? "ring-4 ring-slate-900" : ""}`}
              onClick={() => {
                if (!multi) return onSubmit([o.id], null);
                setPicked((p) => (on ? p.filter((x) => x !== o.id) : [...p, o.id]));
              }}
            >
              <span aria-hidden>{s.shape}</span>
              {o.text}
            </button>
          );
        })}
      </div>
      {multi && (
        <button
          type="button"
          disabled={picked.length === 0}
          onClick={() => onSubmit(picked, null)}
          className="rounded-lg bg-slate-900 px-4 py-3 text-xl font-semibold text-white disabled:opacity-50"
        >
          {t("game.submit")}
        </button>
      )}
    </div>
  );
}
