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
import { Badge, Big, btn, Chip, Countdown, Logo, Muted, Notice, optionStyle, outline, panel, Podium, Scores, Shape, Stage, timeFraction, Waiting, type Tone } from "./ui";
import { asTheme, Avatar, AvatarPicker, randomAvatar, ThemeContext, type Theme } from "./avatars";
import { secondsLeft, useTicker } from "./useCountdown";
import { asChatMode, ChatBar, type ChatMode } from "./chat";
import { hasMedia, MediaZoom, QuestionMedia } from "../media/QuestionMedia";

const DEVICE_KEY = "myquiz.device";
const AVATAR_KEY = "myquiz.avatar";
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
  const [avatar, setAvatar] = useState(() => storage()?.getItem(AVATAR_KEY) ?? randomAvatar());
  const [pinInput, setPinInput] = useState(pin);

  useEffect(() => {
    if (stage.kind !== "checking") return;
    let live = true;
    api<PinCheck>(`/games/pin/${encodeURIComponent(pin)}`)
      .then((game) => {
        if (!live) return;
        // A locked lobby still lets players who are already in the game
        // back in under their name; the server refuses new names.
        if (game.locked) setError("LOCKED_REJOIN");
        setStage({ kind: "nickname", game });
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
        avatar={avatar}
        onLeave={(code) => {
          storage()?.removeItem(tokenKey(pin));
          setError(code);
          setStage({ kind: code === "NICK_TAKEN" || code === "NICK_INVALID" || code === "REMOVED" ? "checking" : "pin" });
        }}
      />
    );
  }

  const input = "w-full rounded-xl border-[3px] border-ink bg-white px-3 py-4 text-center text-2xl font-bold text-ink placeholder:font-medium placeholder:text-ink/40 focus:outline-none focus:ring-4 focus:ring-lagoon";
  const form = `${panel} flex w-full max-w-sm animate-rise flex-col gap-4 p-6`;
  return (
    <Stage
      narrow
      theme={stage.kind === "nickname" ? asTheme(stage.game.theme) : undefined}
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
            storage()?.setItem(AVATAR_KEY, avatar);
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
          <AvatarPicker value={avatar} onChange={setAvatar} label={t("game.pickAvatar")} theme={asTheme(stage.game.theme)} />
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
  /** Banned: this phone cannot come back to this game. */
  | { kind: "banned" }
  /** The name belongs to an offline player; waiting for the host's OK. */
  | { kind: "rejoinWait" };

const REJOIN_RETRY_MS = 3000;

/** Leave codes send the player back to the PIN or nickname screen. */
const FATAL = new Set(["PIN_INVALID", "LOCKED", "KICKED", "BANNED", "FULL", "NICK_TAKEN", "NICK_INVALID", "TOKEN_INVALID", "ALREADY_JOINED"]);

function Game({ pin, nickname, avatar, onLeave }: { pin: string; nickname: string; avatar: string; onLeave: (code: string) => void }) {
  const { t } = useTranslation();
  const [view, setView] = useState<View>({ kind: "connecting" });
  const [me, setMe] = useState<{ nickname: string; avatar: string; score: number; rank: number } | null>(null);
  const [theme, setTheme] = useState<Theme>("cute");
  const [chatMode, setChatMode] = useState<ChatMode>("open");
  // The host decides whether phones show the question and its image/code
  // or only the answer buttons.
  const [showQuestion, setShowQuestion] = useState(true);
  const [zoomed, setZoomed] = useState(false);
  const [muted, setMuted] = useState(false);
  const [status, setStatus] = useState<SocketStatus>("connecting");
  const [submitError, setSubmitError] = useState<string | null>(null);
  const socket = useRef<GameSocket | null>(null);
  // Kept past the question so the result can name the right answer.
  const lastQuestion = useRef<QuestionStart | null>(null);
  const leave = useRef(onLeave);
  leave.current = onLeave;

  const fromSnapshot = useCallback((s: GameSnapshot) => {
    if (s.me) setMe({ nickname: s.me.nickname, avatar: s.me.avatar, score: s.me.score, rank: s.me.rank });
    setTheme(asTheme(s.theme));
    setChatMode(asChatMode(s.chat));
    setShowQuestion(s.phonesShowQuestion !== false);
    setMuted(!!s.me?.muted);
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
        setZoomed(false);
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
      case "game:theme":
        return setTheme(asTheme(p.d.theme));
      case "game:chat":
        return setChatMode(asChatMode(p.d.mode));
      case "game:phones":
        return setShowQuestion(p.d.showQuestion);
      case "player:muted":
        return setMuted(p.d.muted);
      case "player:kicked":
        storage()?.removeItem(tokenKey(pin));
        socket.current?.close();
        // Removed (not banned): back to the name screen to join again
        // under another name.
        if (!p.d?.banned) return leave.current("REMOVED");
        return setView({ kind: "banned" });
      default:
        return;
    }
  }, [pin]);

  useEffect(() => {
    let retry: ReturnType<typeof setTimeout> | undefined;
    const enter = () => {
      clearTimeout(retry);
      const token = storage()?.getItem(tokenKey(pin));
      const attempt = token
        ? s.request<GameSnapshot>({ t: "player:resume", d: { pin, token } })
        : s.request<PlayerJoined>({ t: "player:join", d: { pin, nickname: nickname.trim(), deviceId: deviceId(), avatar } }).then((j) => {
            storage()?.setItem(tokenKey(pin), j.token);
            return j.snapshot;
          });
      attempt.then(fromSnapshot).catch((e) => {
        const code = e instanceof AckError ? e.code : "INTERNAL";
        if (code === "REJOIN_WAIT") {
          // Ask again until the host lets this phone in.
          setView({ kind: "rejoinWait" });
          retry = setTimeout(enter, REJOIN_RETRY_MS);
        } else if (FATAL.has(code)) {
          s.close();
          leave.current(code);
        }
        // Anything else (a timeout, a dropped socket) is retried on the
        // next reconnect.
      });
    };
    const s = new GameSocket({ onPush, onStatus: setStatus, onOpen: enter });
    socket.current = s;
    return () => {
      clearTimeout(retry);
      s.close();
    };
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
  const phoneMedia = playing && showQuestion && view.kind === "question" && hasMedia(view.q) ? view.q : null;
  const tone: Tone | undefined = view.kind === "result" ? (view.r.correct ? "correct" : view.r.answered ? "wrong" : "missed") : undefined;
  const over = view.kind === "ended" || view.kind === "banned";
  // Chat stays out of the way while a player is still answering: it shows
  // once they answered, and then only emoji until the question ends.
  const chatShown = !over && view.kind !== "connecting" && (view.kind !== "question" || view.sent);
  const sendChat = (d: { text?: string; emoji?: string }) =>
    socket.current ? socket.current.request({ t: "player:chat", d }) : Promise.reject(new AckError("DISCONNECTED"));
  const rightAnswer =
    view.kind === "result" && !view.r.correct
      ? (lastQuestion.current?.options ?? [])
          .filter((o) => view.r.correctOptionIds.includes(o.id))
          .map((o) => o.text)
          .join(", ")
      : "";

  return (
    <ThemeContext.Provider value={theme}>
    <Stage
      narrow
      tone={tone}
      theme={theme}
      left={
        me && !over ? (
          <span className="flex min-w-0 items-center gap-2 font-semibold">
            <Avatar id={me.avatar} size={36} />
            <span className="truncate">{me.nickname}</span>
          </span>
        ) : undefined
      }
      right={me && !over && <Chip>{t("game.points", { count: me.score })}</Chip>}
      footer={chatShown && chatMode !== "off" && <ChatBar mode={chatMode} muted={muted} textAllowed={view.kind !== "question"} send={sendChat} />}
    >
      {status === "reconnecting" && <Notice kind="warn">{t("game.reconnecting")}</Notice>}
      {submitError && <Notice kind="error">{t(`game.errors.${submitError}`, { defaultValue: t("errors.INTERNAL") })}</Notice>}

      {view.kind === "connecting" && <Muted>{t("common.loading")}</Muted>}
      {view.kind === "lobby" && (
        <>
          <span className="animate-pop">{me ? <Avatar id={me.avatar} size={120} /> : <Badge>✓</Badge>}</span>
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
            {showQuestion ? <Big>{view.q.text}</Big> : <Muted>{t("game.phones.lookUp")}</Muted>}
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
            {showQuestion ? (
              <p className="min-w-0 flex-1 text-lg font-bold leading-snug">{view.q.text}</p>
            ) : (
              <p className="min-w-0 flex-1 font-semibold text-ink/70">{t("game.phones.lookUp")}</p>
            )}
          </div>
          {phoneMedia && <QuestionMedia value={phoneMedia} maxClass="max-h-[32vh]" codeClass="md-phone" fit={[11, 16]} onZoom={() => setZoomed(true)} />}
          <AnswerPad q={view.q} onSubmit={(ids, text) => submit(view.q, ids, text)} />
        </div>
      )}

      {view.kind === "result" && (
        <>
          <Badge>{!view.r.answered ? "⏱" : view.r.correct ? "✓" : "✕"}</Badge>
          <Big>{!view.r.answered ? t("game.timeUp") : view.r.correct ? t("game.correct") : t("game.incorrect")}</Big>
          <p className="text-6xl font-black tabular-nums">+{view.r.points}</p>
          <p className="rounded-full border-2 border-ink bg-white px-4 py-2 font-bold">{t("game.rankIs", { rank: view.r.rank })}</p>
          {rightAnswer && <p className="text-lg text-ink/90">{t("game.acceptedAnswers", { answers: rightAnswer })}</p>}
        </>
      )}

      {view.kind === "leaderboard" && (
        <>
          <Big>{t("game.leaderboard")}</Big>
          <Scores top={view.top} me={me?.nickname} />
          {me && <p className="rounded-full border-2 border-ink bg-white px-4 py-2 font-bold">{t("game.rankIs", { rank: me.rank })}</p>}
        </>
      )}
      {view.kind === "podium" && (
        <>
          <Big>{t("game.podium")}</Big>
          <Podium top={view.top} me={me?.nickname} />
          {me && <p className="rounded-full border-2 border-ink bg-white px-4 py-2 font-bold">{t("game.finalRank", { rank: me.rank, count: me.score })}</p>}
        </>
      )}
      {view.kind === "ended" && (
        <>
          <Big>{t(`game.ended.${view.reason}`, { defaultValue: t("game.ended.finished") })}</Big>
          {me && <p className="rounded-full border-2 border-ink bg-white px-4 py-2 font-bold">{t("game.finalRank", { rank: me.rank, count: me.score })}</p>}
          <Link to="/" className={btn.secondary}>
            {t("home.join")}
          </Link>
        </>
      )}
      {view.kind === "banned" && <Big>{t("game.errors.BANNED")}</Big>}
      {view.kind === "rejoinWait" && (
        <>
          <Big>{t("game.rejoin.waitTitle")}</Big>
          <Muted>{t("game.rejoin.waitBody")}</Muted>
          <Waiting />
          <button
            type="button"
            className={btn.secondary}
            onClick={() => {
              socket.current?.close();
              leave.current("NICK_TAKEN");
            }}
          >
            {t("game.rejoin.otherName")}
          </button>
        </>
      )}
    </Stage>
    {zoomed && phoneMedia && <MediaZoom value={phoneMedia} onClose={() => setZoomed(false)} />}
    </ThemeContext.Provider>
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
          className="w-full rounded-xl border-[3px] border-ink bg-white px-4 py-4 text-xl font-semibold text-ink placeholder:font-normal placeholder:text-ink/40 focus:outline-none focus:ring-4 focus:ring-lagoon"
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
              className={`relative flex min-h-24 flex-col items-center justify-center gap-3 rounded-2xl p-4 text-xl font-extrabold transition active:translate-y-[3px] active:shadow-[0_1px_0_var(--color-ink)] ${outline} ${optionStyle(i).bg} ${
                multi && on ? "ring-4 ring-sun ring-offset-2 ring-offset-paper" : ""
              }`}
              onClick={() => {
                if (!multi) return onSubmit([o.id], null);
                setPicked((p) => (on ? p.filter((x) => x !== o.id) : [...p, o.id]));
              }}
            >
              <Shape i={i} large />
              <span className="max-w-full break-words">{o.text}</span>
              {multi && on && (
                <span aria-hidden className="absolute right-3 top-3 flex h-7 w-7 items-center justify-center rounded-full border-2 border-ink bg-white text-leaf-deep">
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
