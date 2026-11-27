// Players' chat: a short text or a quick emoji that pops up on the host
// screen for a few seconds. The ids must match `CHAT_EMOJIS` in
// crates/protocol/src/ws.rs; the server only relays ids from that list, so
// the projector never shows anything but these.
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ChatMessage } from "../generated/ChatMessage";
import { Avatar } from "./avatars";

export const CHAT_EMOJIS: Record<string, string> = {
  thumbs: "👍",
  heart: "❤️",
  laugh: "😂",
  wow: "😮",
  party: "🎉",
  fire: "🔥",
  clap: "👏",
  think: "🤔",
};
export const CHAT_EMOJI_IDS = Object.keys(CHAT_EMOJIS);

/** Longest text, as `CHAT_MAX_CHARS` on the server. */
export const CHAT_MAX_CHARS = 60;
/** As the server's cooldowns, so the buttons rest instead of failing. */
export const CHAT_TEXT_COOLDOWN_MS = 4_000;
export const CHAT_EMOJI_COOLDOWN_MS = 1_000;

export type ChatMode = "open" | "emoji" | "off";
export const CHAT_MODES: ChatMode[] = ["open", "emoji", "off"];

export function asChatMode(m: string | null | undefined): ChatMode {
  return m === "emoji" || m === "off" ? m : "open";
}

// ---- Host screen -----------------------------------------------------------

/** How long a popup stays, and how many texts show at once. */
const BUBBLE_MS = 6_000;
const FLOAT_MS = 3_000;
const MAX_BUBBLES = 4;
const MAX_FLOATS = 12;

type Bubble = { key: number; nickname: string; avatar: string; text: string };
type Float = { key: number; emoji: string; left: number };

/**
 * Keeps the popups for the host screen. Texts become bubbles (the newest
 * few; older ones go early); emoji float up along the edge and never stack
 * into a wall.
 */
export function useChatPopups() {
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [floats, setFloats] = useState<Float[]>([]);
  const next = useRef(1);
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());

  useEffect(() => {
    const all = timers.current;
    return () => {
      for (const id of all) clearTimeout(id);
    };
  }, []);

  const later = (fn: () => void, ms: number) => {
    const id = setTimeout(() => {
      timers.current.delete(id);
      fn();
    }, ms);
    timers.current.add(id);
  };

  const add = useCallback((m: ChatMessage) => {
    const key = next.current++;
    if (m.text) {
      const b = { key, nickname: m.nickname, avatar: m.avatar, text: m.text };
      setBubbles((all) => [...all, b].slice(-MAX_BUBBLES));
      later(() => setBubbles((all) => all.filter((x) => x.key !== key)), BUBBLE_MS);
    } else if (m.emoji && CHAT_EMOJIS[m.emoji]) {
      const f = { key, emoji: CHAT_EMOJIS[m.emoji], left: Math.round(Math.random() * 70) };
      setFloats((all) => [...all, f].slice(-MAX_FLOATS));
      later(() => setFloats((all) => all.filter((x) => x.key !== key)), FLOAT_MS);
    }
  }, []);

  const clear = useCallback(() => {
    setBubbles([]);
    setFloats([]);
  }, []);

  return { bubbles, floats, add, clear };
}

/**
 * The popups, over the bottom-left corner of the host screen where the
 * question and answer tiles are not. Read-only and ignored by pointer and
 * screen readers' live regions, so they never get in the host's way.
 */
export function ChatPopups({ bubbles, floats, compact }: { bubbles: Bubble[]; floats: Float[]; compact?: boolean }) {
  return (
    <div aria-hidden className="pointer-events-none fixed bottom-20 left-3 z-30 flex w-[min(26rem,40vw)] flex-col items-start gap-2 sm:left-5">
      <div className="relative h-0 w-24 self-start">
        {floats.map((f) => (
          <span key={f.key} className={`absolute bottom-0 animate-float-up drop-shadow ${compact ? "text-2xl opacity-80" : "text-4xl"}`} style={{ left: `${f.left}px` }}>
            {f.emoji}
          </span>
        ))}
      </div>
      {bubbles.slice(compact ? -2 : -MAX_BUBBLES).map((b) => (
        <div
          key={b.key}
          data-testid="chat-bubble"
          className="flex max-w-full animate-bubble items-start gap-2 rounded-2xl rounded-bl-sm border-[3px] border-ink bg-white px-2.5 py-1.5 text-left shadow-[0_3px_0_var(--color-ink)]"
        >
          <Avatar id={b.avatar} size={30} />
          <div className="min-w-0">
            <p className="truncate text-xs font-extrabold text-ink/60">{b.nickname}</p>
            <p className="break-words text-lg font-bold leading-snug">{b.text}</p>
          </div>
        </div>
      ))}
    </div>
  );
}

/** The host's switch: everyone may chat, emoji only, or nobody. */
export function ChatModePicker({ value, disabled, onChange }: { value: ChatMode; disabled?: boolean; onChange: (m: ChatMode) => void }) {
  const { t } = useTranslation();
  return (
    <div role="radiogroup" aria-label={t("game.chat.label")} className="flex items-center gap-1 rounded-2xl border-[3px] border-ink bg-white p-1 shadow-[0_3px_0_var(--color-ink)]">
      <span aria-hidden className="pl-1.5 pr-0.5 text-base">
        💬
      </span>
      {CHAT_MODES.map((m) => (
        <button
          key={m}
          type="button"
          role="radio"
          aria-checked={value === m}
          disabled={disabled}
          onClick={() => value !== m && onChange(m)}
          className={`rounded-xl px-2.5 py-1 text-sm font-bold transition ${value === m ? "bg-sand" : "hover:bg-mint"}`}
        >
          {t(`game.chat.mode.${m}`)}
        </button>
      ))}
    </div>
  );
}

// ---- Player's phone ----------------------------------------------------------

/**
 * The player's chat bar: a row of quick emoji and, when texts are allowed
 * right now, a one-line box. `send` resolves when the server took it and
 * rejects with the refusal code.
 */
export function ChatBar({ mode, textAllowed, send }: { mode: ChatMode; textAllowed: boolean; send: (msg: { text?: string; emoji?: string }) => Promise<unknown> }) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [note, setNote] = useState<{ kind: "ok" | "error"; code: string } | null>(null);
  const [restUntil, setRestUntil] = useState({ text: 0, emoji: 0 });
  const [, tick] = useState(0);
  const noteTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (noteTimer.current) clearTimeout(noteTimer.current);
  }, []);

  // Redraw when a cooldown ends, so the buttons come back.
  useEffect(() => {
    const wait = Math.max(restUntil.text, restUntil.emoji) - Date.now();
    if (wait <= 0) return;
    const id = setTimeout(() => tick((n) => n + 1), wait + 20);
    return () => clearTimeout(id);
  }, [restUntil]);

  if (mode === "off") return null;
  const now = Date.now();
  const emojiResting = now < restUntil.emoji;
  const textResting = now < restUntil.text;
  const showText = mode === "open" && textAllowed;

  function flash(kind: "ok" | "error", code: string) {
    setNote({ kind, code });
    if (noteTimer.current) clearTimeout(noteTimer.current);
    noteTimer.current = setTimeout(() => setNote(null), 2500);
  }

  async function go(msg: { text?: string; emoji?: string }) {
    const kind = msg.text ? "text" : "emoji";
    const rest = kind === "text" ? CHAT_TEXT_COOLDOWN_MS : CHAT_EMOJI_COOLDOWN_MS;
    setRestUntil((r) => ({ ...r, [kind]: Date.now() + rest }));
    try {
      await send(msg);
      if (msg.text) setText("");
      flash("ok", "SENT");
    } catch (e) {
      const code = e instanceof Error ? e.message : "INTERNAL";
      flash("error", code);
    }
  }

  return (
    <div className="flex w-full flex-col gap-2" data-testid="chat-bar">
      <div role="group" aria-label={t("game.chat.react")} className="flex w-full justify-between gap-1">
        {CHAT_EMOJI_IDS.map((id) => (
          <button
            key={id}
            type="button"
            disabled={emojiResting}
            aria-label={t(`game.chat.emoji.${id}`)}
            onClick={() => void go({ emoji: id })}
            className="flex h-10 w-10 items-center justify-center rounded-full border-2 border-ink/20 bg-white text-xl transition active:scale-90 disabled:opacity-40"
          >
            {CHAT_EMOJIS[id]}
          </button>
        ))}
      </div>
      {showText && (
        <form
          className="flex w-full gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const v = text.trim();
            if (v && !textResting) void go({ text: v });
          }}
        >
          <input
            value={text}
            maxLength={CHAT_MAX_CHARS}
            onChange={(e) => setText(e.target.value)}
            aria-label={t("game.chat.placeholder")}
            placeholder={t("game.chat.placeholder")}
            enterKeyHint="send"
            className="min-w-0 flex-1 rounded-xl border-2 border-ink bg-white px-3 py-2 text-base text-ink placeholder:text-ink/40 focus:outline-none focus:ring-4 focus:ring-lagoon/50"
          />
          <button type="submit" disabled={!text.trim() || textResting} className="rounded-xl border-2 border-ink bg-sea px-3 py-2 text-sm font-bold text-white disabled:opacity-40">
            {t("game.chat.send")}
          </button>
        </form>
      )}
      <p className={`h-4 text-xs ${note?.kind === "error" ? "text-red-700" : "text-ink/60"}`} aria-live="polite">
        {note ? t(`game.chat.notes.${note.code}`, { defaultValue: t("game.chat.notes.INTERNAL") }) : mode === "emoji" ? t("game.chat.emojiOnly") : !textAllowed && mode === "open" ? t("game.chat.duringQuestion") : ""}
      </p>
    </div>
  );
}
