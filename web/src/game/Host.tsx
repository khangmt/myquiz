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
import type { RejoinRequest } from "../generated/RejoinRequest";
import { AckError, GameSocket, type SocketStatus } from "./socket";
import { JoinQr } from "./JoinQr";
import { Big, btn, Chip, Countdown, Muted, Notice, optionStyle, outline, panel, Podium, Scores, Shape, Stage, timeFraction, Waiting } from "./ui";
import { asTheme, Avatar, THEMES, ThemeContext, type Theme } from "./avatars";
import { MusicControl } from "./MusicControl";
import { secondsLeft, useTicker } from "./useCountdown";
import type { SessionReport } from "../generated/SessionReport";
import { asChatMode, ChatModePicker, ChatPopups, useChatPopups, type ChatMode } from "./chat";
import { hasMedia, MediaZoom, QuestionMedia } from "../media/QuestionMedia";
import { openMediaWindow, useMediaBroadcast } from "../media/popout";

/** The host's last choice of what phones show, kept for their next game. */
const PHONES_KEY = "myquiz.host.phonesShowQuestion";
function phonesPreference(): boolean | null {
  try {
    const v = window.localStorage.getItem(PHONES_KEY);
    return v === null ? null : v === "1";
  } catch {
    return null;
  }
}
function savePhonesPreference(show: boolean) {
  try {
    window.localStorage.setItem(PHONES_KEY, show ? "1" : "0");
  } catch {
    // Private mode: the choice lasts for this game only.
  }
}
import { LobbyPlayerChip, PlayersDialog } from "./players";

/** The lobby's "game name" box (e.g. the class), saved to the report as the
 *  host types, so games of one quiz with different classes stay apart. */
function GameName({ reportId }: { reportId: string }) {
  const { t } = useTranslation();
  const [name, setName] = useState<string | null>(null);
  const [state, setState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const saved = useRef("");
  useEffect(() => {
    let live = true;
    api<SessionReport>(`/sessions/${reportId}`)
      .then((r) => {
        if (!live) return;
        saved.current = r.session.name ?? "";
        setName((n) => n ?? saved.current);
      })
      .catch(() => live && setName((n) => n ?? ""));
    return () => {
      live = false;
    };
  }, [reportId]);
  async function save() {
    const value = (name ?? "").trim();
    if (value === saved.current) return;
    setState("saving");
    try {
      await api(`/sessions/${reportId}`, { method: "PATCH", json: { name: value || null } });
      saved.current = value;
      setState("saved");
    } catch {
      setState("error");
    }
  }
  return (
    <form
      className="flex w-full max-w-md flex-col gap-1"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <label htmlFor="game-name" className="text-sm text-ink/70">
        {t("game.nameLabel")}
      </label>
      <input
        id="game-name"
        value={name ?? ""}
        maxLength={80}
        placeholder={t("game.namePlaceholder")}
        onChange={(e) => {
          setName(e.target.value);
          setState("idle");
        }}
        onBlur={() => void save()}
        className="rounded-lg border-2 border-ink bg-white px-3 py-2 text-ink placeholder:text-ink/40 focus:ring-4 focus:ring-lagoon/50 focus:outline-none"
      />
      <span className="h-4 text-xs text-ink/60" aria-live="polite">
        {state === "saving" ? t("editor.saving") : state === "saved" ? t("game.nameSaved") : state === "error" ? t("errors.INTERNAL") : ""}
      </span>
    </form>
  );
}

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
  const [theme, setTheme] = useState<Theme>("cute");
  const [chatMode, setChatMode] = useState<ChatMode>("open");
  // Whether phones show the question and its image/code, or only the
  // answer buttons (when everyone reads the projector).
  const [phonesShow, setPhonesShow] = useState(true);
  // Exam mode: the game moves on by itself after each question.
  const [exam, setExam] = useState(false);
  // The question's image or code, enlarged over the whole screen.
  const [zoomed, setZoomed] = useState(false);
  // Hides the popups on this screen only; players can still send. For when
  // the host wants a clean projector for a moment without telling the room.
  const [chatHidden, setChatHidden] = useState(false);
  const chat = useChatPopups();
  const { add: addChat, clear: clearChat } = chat;
  const hiddenRef = useRef(false);
  hiddenRef.current = chatHidden;
  // Players on a new device asking to continue under their name.
  const [rejoins, setRejoins] = useState<RejoinRequest[]>([]);
  // The list of players, to mute, remove or ban one mid-game.
  const [managing, setManaging] = useState(false);

  const fromSnapshot = useCallback((s: GameSnapshot) => {
    state.current = s.state;
    if (s.reportId) setReportId(s.reportId);
    setPlayers(s.players);
    setLocked(s.locked);
    setTheme(asTheme(s.theme));
    setChatMode(asChatMode(s.chat));
    setExam(!!s.exam);
    const shown = s.phonesShowQuestion !== false;
    setPhonesShow(shown);
    // A new game starts with the host's choice from last time.
    const pref = phonesPreference();
    if (s.state.state === "lobby" && pref !== null && pref !== shown) {
      setPhonesShow(pref);
      socket.current?.request({ t: "host:phones", d: { showQuestion: pref } }).catch(() => setPhonesShow(shown));
    }
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
        // A new question starts on a clean screen.
        clearChat();
        setZoomed(false);
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
      case "game:theme":
        return setTheme(asTheme(p.d.theme));
      case "game:chat":
        return setChatMode(asChatMode(p.d.mode));
      case "game:phones":
        return setPhonesShow(p.d.showQuestion);
      case "game:mode":
        return setExam(p.d.exam);
      case "chat:message":
        if (!hiddenRef.current) addChat(p.d);
        return;
      case "rejoin:request": {
        const req = p.d;
        return setRejoins((rs) => (rs.some((r) => r.playerId === req.playerId) ? rs : [...rs, req]));
      }
      default:
        return;
    }
  }, [addChat, clearChat]);

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

  // When an exam-mode reveal moves on by itself, if this one does.
  const advancesAt = view.kind === "reveal" ? (view.reveal.advancesAt ?? null) : null;
  const now = useTicker(view.kind === "question" || advancesAt !== null, useCallback(() => socket.current?.serverNow() ?? Date.now(), []));

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
  // The image or code on screen now, if any. The pop-out window keeps the
  // last one through the leaderboard, so the class can still discuss it.
  const shownMedia = current && hasMedia(current) ? current : null;
  const windowQ = current ?? (view.kind === "leaderboard" ? lastQuestion.current : null);
  useMediaBroadcast(
    pin,
    windowQ ? { media: windowQ.media, markdown: windowQ.markdown, text: windowQ.text, index: windowQ.index, total: windowQ.total } : null,
  );
  const canZoom = !!shownMedia && (view.kind === "reveal" || started);
  useEffect(() => {
    if (!canZoom) {
      setZoomed(false);
      return;
    }
    // Z enlarges the image or code (Esc closes it), for a host at a laptop
    // or with a presenter remote that sends keys.
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "z" || e.key === "Z") {
        e.preventDefault();
        setZoomed((z) => !z);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [canZoom]);
  const zoom = () => setZoomed(true);

  const nextButton = (label: string) => (
    <button type="button" className={btn.primary} disabled={busy} onClick={next}>
      {label}
    </button>
  );

  return (
    <ThemeContext.Provider value={theme}>
    <Stage
      theme={theme}
      center={current && t("game.questionOf", { n: current.index + 1, total: current.total })}
      right={
        live && (
            <>
              <MusicControl />
              {view.kind !== "lobby" && (
                <>
              {exam && <Chip>{t("game.mode.badge")}</Chip>}
              <Chip>
                <span className="text-ink/60">PIN</span>
                <span className="tracking-wider">{pin}</span>
              </Chip>
              <button type="button" aria-label={t("game.players.title")} aria-haspopup="dialog" className="rounded-full transition hover:brightness-95" onClick={() => setManaging(true)}>
                <Chip>
                  <PersonIcon />
                  {players.length}
                </Chip>
              </button>
                </>
              )}
            </>
          )
      }
      footer={
        live && (
          <>
            {confirmEnd ? (
              <div className="flex items-center gap-2">
                <span className="text-sm text-ink/80">{t("game.confirmEnd")}</span>
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
              {shownMedia && (
                <button type="button" className={btn.ghost} onClick={() => openMediaWindow(pin)} title={t("media.popOutHint")}>
                  {t("media.popOut")} ↗
                </button>
              )}
              <button
                type="button"
                className={btn.ghost}
                aria-pressed={!phonesShow}
                title={t("game.phones.hint")}
                onClick={() => {
                  const show = !phonesShow;
                  savePhonesPreference(show);
                  setPhonesShow(show);
                  void act({ t: "host:phones", d: { showQuestion: show } });
                }}
              >
                <PhoneIcon />
                {phonesShow ? t("game.phones.hide") : t("game.phones.show")}
              </button>
              <ChatModePicker value={chatMode} onChange={(m) => act({ t: "host:chat", d: { mode: m } })} />
              {chatMode !== "off" && (
                <button
                  type="button"
                  className={btn.ghost}
                  aria-pressed={chatHidden}
                  onClick={() => {
                    setChatHidden(!chatHidden);
                    chat.clear();
                  }}
                >
                  {chatHidden ? t("game.chat.show") : t("game.chat.hide")}
                </button>
              )}
              {view.kind === "lobby" && (
                <>
                  <ModePicker exam={exam} disabled={busy} onChange={(e) => act({ t: "host:mode", d: { exam: e } })} />
                  <ThemePicker value={theme} disabled={busy} onChange={(th) => act({ t: "host:theme", d: { theme: th } })} />
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
              {advancesAt !== null && view.kind === "reveal" && (
                <span className="font-bold text-ink/80" aria-live="polite">
                  {t(view.q && view.q.index + 1 >= view.q.total ? "game.mode.podiumIn" : "game.mode.nextIn", { s: secondsLeft(advancesAt, now) })}
                </span>
              )}
              {(view.kind === "reveal" || view.kind === "leaderboard" || view.kind === "between") &&
                nextButton(advancesAt !== null ? t("game.mode.nextNow") : t("game.next"))}
              {view.kind === "podium" && nextButton(t("game.finish"))}
            </div>
          </>
        )
      }
    >
      {live && managing && view.kind !== "lobby" && <PlayersDialog players={players} act={act} onClose={() => setManaging(false)} />}
      {live && !chatHidden && chatMode !== "off" && (
        // Fewer texts at once while a question is up (only emoji arrive then).
        <ChatPopups bubbles={chat.bubbles} floats={chat.floats} compact={view.kind === "question"} />
      )}
      {status === "reconnecting" && <Notice kind="warn">{t("game.reconnecting")}</Notice>}
      {live &&
        rejoins.map((r) => {
          const dismiss = () => setRejoins((rs) => rs.filter((x) => x.playerId !== r.playerId));
          return (
            <div key={r.playerId} role="status" className={`${panel} flex w-full flex-wrap items-center gap-3 px-4 py-3`}>
              <Avatar id={r.avatar} size={40} />
              <span className="min-w-0 flex-1 font-semibold">{t("game.rejoin.request", { name: r.nickname })}</span>
              <button
                type="button"
                className={btn.danger}
                onClick={() => {
                  dismiss();
                  void act({ t: "host:rejoin", d: { playerId: r.playerId } });
                }}
              >
                {t("game.rejoin.allow")}
              </button>
              <button type="button" className={btn.ghost} onClick={dismiss}>
                {t("game.rejoin.ignore")}
              </button>
            </div>
          );
        })}
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
            <p className="text-lg text-ink/70">{t("game.joinAt", { url: `${joinBase.replace(/^https?:\/\//, "")}/join` })}</p>
            <p className="font-mono text-6xl font-black tracking-[0.15em] text-sea sm:text-7xl xl:text-8xl" aria-label={t("game.pinIs", { pin })}>
              {pin}
            </p>
            <JoinQr url={joinUrl} label={t("game.qr")} />
            <p className="break-all text-xs text-ink/40">{joinUrl}</p>
            {reportId && <GameName reportId={reportId} />}
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
                  <LobbyPlayerChip key={p.id} player={p} act={act} />
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
            <div className={`grid items-center gap-4 sm:gap-8 ${shownMedia ? "flex-1 grid-cols-[auto_minmax(0,1fr)_auto]" : "grid-cols-[1fr_auto_1fr]"}`}>
              <div className="flex justify-start">
                <Countdown large seconds={secondsLeft(view.q.endsAt, now)} fraction={timeFraction(view.q, now)} />
              </div>
              {shownMedia ? <QuestionMedia value={shownMedia} maxClass="max-h-[42vh]" onZoom={zoom} /> : <span />}
              <p className="justify-self-end rounded-2xl border-[3px] border-ink bg-white px-4 py-3 text-xl font-black tabular-nums sm:text-2xl" aria-live="polite">
                {t("game.answered", { answered: view.answered, connected: view.connected })}
              </p>
            </div>
            <OptionGrid q={view.q} fill={!shownMedia} />
          </div>
        ))}

      {view.kind === "reveal" && (
        <div className="flex w-full flex-1 flex-col justify-center gap-6">
          {view.q && <QuestionText>{view.q.text}</QuestionText>}
          {shownMedia ? (
            // The image or code stays up beside the answers, so the class
            // can talk through why the answer is right.
            <div className="grid w-full items-center gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
              <QuestionMedia value={shownMedia} maxClass="max-h-[55vh]" onZoom={zoom} />
              <RevealView q={view.q} reveal={view.reveal} stacked />
            </div>
          ) : (
            <RevealView q={view.q} reveal={view.reveal} />
          )}
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
    {zoomed && shownMedia && <MediaZoom value={shownMedia} onClose={() => setZoomed(false)} onPopOut={() => openMediaWindow(pin)} />}
    </ThemeContext.Provider>
  );
}

/** Normal (the host presses Next) or exam (moves on by itself), picked in the lobby. */
function ModePicker({ exam, disabled, onChange }: { exam: boolean; disabled: boolean; onChange: (exam: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <div role="radiogroup" aria-label={t("game.mode.label")} title={t("game.mode.hint")} className="flex items-center gap-1 rounded-2xl border-[3px] border-ink bg-white p-1 shadow-[0_3px_0_var(--color-ink)]">
      {[false, true].map((e) => (
        <button
          key={String(e)}
          type="button"
          role="radio"
          aria-checked={exam === e}
          disabled={disabled}
          onClick={() => exam !== e && onChange(e)}
          className={`rounded-xl px-3 py-1.5 text-sm font-bold transition ${exam === e ? "bg-sand" : "hover:bg-mint"}`}
        >
          {t(e ? "game.mode.exam" : "game.mode.normal")}
        </button>
      ))}
    </div>
  );
}

/** Cute or anime: the look of the avatars and stage, picked in the lobby. */
function ThemePicker({ value, disabled, onChange }: { value: Theme; disabled: boolean; onChange: (t: Theme) => void }) {
  const { t } = useTranslation();
  return (
    <div role="radiogroup" aria-label={t("game.theme.label")} className="flex items-center gap-1 rounded-2xl border-[3px] border-ink bg-white p-1 shadow-[0_3px_0_var(--color-ink)]">
      {THEMES.map((th) => (
        <button
          key={th}
          type="button"
          role="radio"
          aria-checked={value === th}
          disabled={disabled}
          onClick={() => value !== th && onChange(th)}
          className={`flex items-center gap-1.5 rounded-xl py-1 pl-1 pr-3 text-sm font-bold transition ${value === th ? "bg-sand" : "hover:bg-mint"}`}
        >
          <Avatar id="bunny" theme={th} size={28} />
          {t(`game.theme.${th}`)}
        </button>
      ))}
    </div>
  );
}

function PhoneIcon() {
  return (
    <svg aria-hidden viewBox="0 0 20 20" fill="currentColor" className="mr-1.5 h-[1.1em] w-[1.1em] opacity-70">
      <path d="M6 1.5h8A1.5 1.5 0 0 1 15.5 3v14a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 17V3A1.5 1.5 0 0 1 6 1.5Zm1 2v11h6v-11H7Zm3 12.25a.9.9 0 1 0 0 1.8.9.9 0 0 0 0-1.8Z" />
    </svg>
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
function OptionGrid({ q, correct, counts, fill, stacked }: { q: QuestionStart; correct?: number[]; counts?: Map<number, number>; fill?: boolean; stacked?: boolean }) {
  const { t } = useTranslation();
  if (q.type === "fill_blank") return null;
  const max = counts ? Math.max(1, ...counts.values()) : 1;
  return (
    <ul className={`grid w-full grid-cols-1 gap-3 ${stacked ? "" : "sm:grid-cols-2"} ${fill ? "flex-1 auto-rows-fr" : ""}`} aria-label={counts ? t("game.distribution") : undefined}>
      {q.options.map((o, i) => {
        const right = correct?.includes(o.id);
        const n = counts?.get(o.id) ?? 0;
        return (
          <li
            key={o.id}
            className={`flex flex-col justify-center gap-3 rounded-2xl p-4 text-left text-xl font-extrabold transition sm:p-5 sm:text-2xl ${outline} ${optionStyle(i).bg} ${
              correct ? (right ? "ring-4 ring-sun ring-offset-2 ring-offset-paper" : "opacity-45 saturate-50") : ""
            }`}
          >
            <div className="flex items-center gap-3">
              <Shape i={i} />
              <span className="min-w-0 flex-1 break-words">{o.text}</span>
              {right && (
                <span aria-hidden className="flex h-9 w-9 shrink-0 animate-pop items-center justify-center rounded-full border-2 border-ink bg-white text-xl text-leaf-deep">
                  ✓
                </span>
              )}
              {counts && <span className="shrink-0 text-3xl font-black tabular-nums">{n}</span>}
            </div>
            {counts && (
              <div aria-hidden className="h-3 w-full overflow-hidden rounded-full border-2 border-current bg-white/30">
                <div className="h-full origin-left rounded-full bg-current transition-[width] duration-700" style={{ width: `${(n / max) * 100}%` }} />
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

function RevealView({ q, reveal, stacked }: { q: QuestionStart | null; reveal: QuestionReveal; stacked?: boolean }) {
  const { t } = useTranslation();
  if (reveal.acceptedAnswers.length > 0 || q?.type === "fill_blank") {
    return (
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-4">
        <p className={`animate-pop rounded-2xl bg-leaf-deep px-6 py-5 text-2xl font-extrabold text-white sm:text-3xl ${outline}`}>
          {t("game.acceptedAnswers", { answers: reveal.acceptedAnswers.join(", ") })}
        </p>
        <ul className="flex flex-col gap-2">
          {reveal.topTextAnswers.map((a) => (
            <li key={a.text} className={`flex items-center gap-3 rounded-xl border-2 px-4 py-2 text-lg ${a.correct ? "border-ink bg-mint font-bold" : "border-ink/20 bg-white/80 text-ink/70"}`}>
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
  return <OptionGrid q={q} correct={reveal.correctOptionIds} counts={counts} stacked={stacked} />;
}
