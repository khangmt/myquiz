import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { api, ApiFailure } from "../api/client";
import type { GameSnapshot } from "../generated/GameSnapshot";
import type { PinCheck } from "../generated/PinCheck";
import type { PlayerJoined } from "../generated/PlayerJoined";
import type { QuestionResult } from "../generated/QuestionResult";
import type { QuestionStart } from "../generated/QuestionStart";
import type { ScoreRow } from "../generated/ScoreRow";
import type { ServerPush } from "../generated/ServerPush";
import { LanguageSelect } from "../components/LanguageSelect";
import { AckError, GameSocket, type SocketStatus } from "./socket";
import { Badge, Big, btn, Chip, Countdown, Logo, Muted, Notice, optionStyle, panel, Podium, Scores, Shape, Stage, timeFraction, Waiting, type Tone } from "./ui";
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

  const input = "w-full rounded-xl border-0 bg-white px-3 py-4 text-center text-2xl font-bold text-slate-900 placeholder:font-medium placeholder:text-slate-400 focus:outline-none focus:ring-4 focus:ring-indigo-400";
  const form = `${panel} flex w-full max-w-sm animate-rise flex-col gap-4 p-6`;
  return (
    <Stage
      narrow
      left={
        <Link to="/" aria-label={t("app.name")}>
          <Logo />
        </Link>
      }
      right={<LanguageSelect />}
    >
      {error && <Notice kind="error">{t(`game.errors.${error}`, { defaultValue: t("errors.INTERNAL") })}</Notice>}
      {stage.kind === "checking" && <Muted>{t("common.loading")}</Muted>}
      {stage.kind === "pin" && (
        <form
          className={form}
          onSubmit={(e) => {
            e.preventDefault();
            if (pinInput.length !== 6) return;
            setError(null);
            setParams({ pin: pinInput });
            setStage({ kind: "checking" });
          }}
        >
          <input
            className={`${input} ${pinInput ? "tracking-[0.3em]" : ""}`}
            inputMode="numeric"
            autoComplete="off"
            autoFocus
            maxLength={6}
            aria-label={t("home.pinPlaceholder")}
            placeholder={t("home.pinPlaceholder")}
            value={pinInput}
            onChange={(e) => setPinInput(e.target.value.replace(/\D/g, ""))}
          />
          <button className={btn.primary} type="submit" disabled={pinInput.length !== 6}>
            {t("home.join")}
          </button>
        </form>
      )}
      {stage.kind === "nickname" && (
        <form
          className={form}
          onSubmit={(e) => {
            e.preventDefault();
            if (!nickname.trim()) return;
            setError(null);
            setStage({ kind: "playing" });
          }}
        >
          <p className="text-2xl font-extrabold text-balance">{stage.game.title}</p>
          <input
            className={input}
            autoFocus
            maxLength={20}
            aria-label={t("game.nickname")}
            placeholder={t("game.nickname")}
            value={nickname}
            onChange={(e) => setNickname(e.target.value)}
          />
          <button className={btn.primary} type="submit" disabled={!nickname.trim()}>
            {t("game.enter")}
          </button>
        </form>
      )}
    </Stage>
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
  // Kept past the question so the result can name the right answer.
  const lastQuestion = useRef<QuestionStart | null>(null);
  const leave = useRef(onLeave);
  leave.current = onLeave;

  const fromSnapshot = useCallback((s: GameSnapshot) => {
    if (s.me) setMe({ nickname: s.me.nickname, score: s.me.score, rank: s.me.rank });
    if (s.question) lastQuestion.current = s.question;
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
        lastQuestion.current = p.d;
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

  const playing = view.kind === "question" && now >= view.q.startsAt && !view.sent && now < view.q.endsAt;
  const tone: Tone | undefined = view.kind === "result" ? (view.r.correct ? "correct" : view.r.answered ? "wrong" : "missed") : undefined;
  const over = view.kind === "ended" || view.kind === "kicked";
  const rightAnswer =
    view.kind === "result" && !view.r.correct
      ? (lastQuestion.current?.options ?? [])
          .filter((o) => view.r.correctOptionIds.includes(o.id))
          .map((o) => o.text)
          .join(", ")
      : "";

  return (
    <Stage
      narrow
      tone={tone}
      left={
        me && !over ? (
          <span className="flex min-w-0 items-center gap-2 font-semibold">
            <span aria-hidden className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-white/15 text-sm font-bold uppercase">
              {[...me.nickname][0]}
            </span>
            <span className="truncate">{me.nickname}</span>
          </span>
        ) : undefined
      }
      right={me && !over && <Chip>{t("game.points", { count: me.score })}</Chip>}
    >
      {status === "reconnecting" && <Notice kind="warn">{t("game.reconnecting")}</Notice>}
      {submitError && <Notice kind="error">{t(`game.errors.${submitError}`, { defaultValue: t("errors.INTERNAL") })}</Notice>}

      {view.kind === "connecting" && <Muted>{t("common.loading")}</Muted>}
      {view.kind === "lobby" && (
        <>
          <Badge>✓</Badge>
          <Big>{t("game.youreIn")}</Big>
          <Muted>{t("game.waitForHost")}</Muted>
          <Waiting />
        </>
      )}
      {view.kind === "waiting" && (
        <>
          <Muted>{t("game.waitForNext")}</Muted>
          <Waiting />
        </>
      )}

      {view.kind === "question" &&
        (now < view.q.startsAt ? (
          <>
            <Chip>{t("game.questionOf", { n: view.q.index + 1, total: view.q.total })}</Chip>
            <Big>{view.q.text}</Big>
            <Muted>{t("game.getReady", { s: secondsLeft(view.q.startsAt, now) })}</Muted>
          </>
        ) : view.sent ? (
          <>
            <Badge>✓</Badge>
            <Big>{t("game.answerSent")}</Big>
            <Muted>{t("game.waitForOthers")}</Muted>
            <Waiting />
          </>
        ) : now >= view.q.endsAt ? (
          <>
            <Badge>⏱</Badge>
            <Big>{t("game.timeUp")}</Big>
          </>
        ) : null)}
      {playing && view.kind === "question" && (
        <div className="flex w-full flex-1 flex-col gap-4 self-stretch">
          <div className={`${panel} flex items-center gap-4 p-3 text-left`}>
            <Countdown seconds={secondsLeft(view.q.endsAt, now)} fraction={timeFraction(view.q, now)} />
            <p className="min-w-0 flex-1 text-lg font-bold leading-snug">{view.q.text}</p>
          </div>
          <AnswerPad q={view.q} onSubmit={(ids, text) => submit(view.q, ids, text)} />
        </div>
      )}

      {view.kind === "result" && (
        <>
          <Badge>{!view.r.answered ? "⏱" : view.r.correct ? "✓" : "✕"}</Badge>
          <Big>{!view.r.answered ? t("game.timeUp") : view.r.correct ? t("game.correct") : t("game.incorrect")}</Big>
          <p className="text-6xl font-black tabular-nums">+{view.r.points}</p>
          <p className="rounded-full bg-black/20 px-4 py-2 font-semibold">{t("game.rankIs", { rank: view.r.rank })}</p>
          {rightAnswer && <p className="text-lg text-white/90">{t("game.acceptedAnswers", { answers: rightAnswer })}</p>}
        </>
      )}

      {view.kind === "leaderboard" && (
        <>
          <Big>{t("game.leaderboard")}</Big>
          <Scores top={view.top} me={me?.nickname} />
          {me && <p className="rounded-full bg-white/10 px-4 py-2 font-semibold">{t("game.rankIs", { rank: me.rank })}</p>}
        </>
      )}
      {view.kind === "podium" && (
        <>
          <Big>{t("game.podium")}</Big>
          <Podium top={view.top} me={me?.nickname} />
          {me && <p className="rounded-full bg-white/10 px-4 py-2 font-semibold">{t("game.finalRank", { rank: me.rank, count: me.score })}</p>}
        </>
      )}
      {view.kind === "ended" && (
        <>
          <Big>{t(`game.ended.${view.reason}`, { defaultValue: t("game.ended.finished") })}</Big>
          {me && <p className="rounded-full bg-white/10 px-4 py-2 font-semibold">{t("game.finalRank", { rank: me.rank, count: me.score })}</p>}
          <Link to="/" className={btn.secondary}>
            {t("home.join")}
          </Link>
        </>
      )}
      {view.kind === "kicked" && <Big>{t("game.errors.KICKED")}</Big>}
    </Stage>
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
          className="w-full rounded-xl border-0 bg-white px-4 py-4 text-xl font-semibold text-slate-900 placeholder:font-normal placeholder:text-slate-400 focus:outline-none focus:ring-4 focus:ring-indigo-400"
          autoFocus
          maxLength={100}
          aria-label={t("game.yourAnswer")}
          placeholder={t("game.yourAnswer")}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <button type="submit" disabled={!text.trim()} className={btn.primary}>
          {t("game.submit")}
        </button>
      </form>
    );
  }

  const multi = q.type === "multi";
  return (
    <div className="flex w-full flex-1 flex-col gap-3">
      {/* The tiles fill the phone, so every answer is an easy thumb target. */}
      <div className={`grid w-full flex-1 auto-rows-fr gap-3 ${q.options.length > 2 ? "grid-cols-2" : "grid-cols-1"}`}>
        {q.options.map((o, i) => {
          const on = picked.includes(o.id);
          return (
            <button
              key={o.id}
              type="button"
              aria-pressed={multi ? on : undefined}
              className={`relative flex min-h-24 flex-col items-center justify-center gap-3 rounded-2xl p-4 text-xl font-bold shadow-lg shadow-black/25 transition active:scale-[0.97] ${optionStyle(i).bg} ${
                multi && on ? "ring-4 ring-white" : ""
              }`}
              onClick={() => {
                if (!multi) return onSubmit([o.id], null);
                setPicked((p) => (on ? p.filter((x) => x !== o.id) : [...p, o.id]));
              }}
            >
              <Shape i={i} large />
              <span className="max-w-full break-words">{o.text}</span>
              {multi && on && (
                <span aria-hidden className="absolute right-3 top-3 flex h-7 w-7 items-center justify-center rounded-full bg-white text-emerald-700">
                  ✓
                </span>
              )}
            </button>
          );
        })}
      </div>
      {multi && (
        <button type="button" disabled={picked.length === 0} onClick={() => onSubmit(picked, null)} className={btn.primary}>
          {t("game.submit")}
        </button>
      )}
    </div>
  );
}
