import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { MusicPlayer, TRACKS } from "./music";

const KEY = "myquiz.music";

type Settings = { track: string | null; volume: number; muted: boolean };

function load(): Settings {
  const fallback: Settings = { track: TRACKS[0].id, volume: 0.6, muted: false };
  try {
    const raw = window.localStorage.getItem(KEY);
    return raw ? { ...fallback, ...(JSON.parse(raw) as Partial<Settings>) } : fallback;
  } catch {
    return fallback;
  }
}

/**
 * The host's music picker: a track (or off), mute and volume. Settings are
 * remembered on this computer. Music starts with the host's first click,
 * since browsers block sound until then.
 */
export function MusicControl() {
  const { t } = useTranslation();
  const [s, setS] = useState<Settings>(load);
  const player = useRef<MusicPlayer | null>(null);

  useEffect(() => {
    if (!MusicPlayer.supported()) return;
    const p = new MusicPlayer();
    player.current = p;
    const unlock = () => p.unlock();
    window.addEventListener("pointerdown", unlock);
    window.addEventListener("keydown", unlock);
    return () => {
      window.removeEventListener("pointerdown", unlock);
      window.removeEventListener("keydown", unlock);
      p.dispose();
      player.current = null;
    };
  }, []);

  useEffect(() => {
    const p = player.current;
    if (p) {
      p.setVolume(s.volume);
      p.setMuted(s.muted);
      p.play(s.track);
    }
    try {
      window.localStorage.setItem(KEY, JSON.stringify(s));
    } catch {
      // Private mode: the settings just aren't remembered.
    }
  }, [s]);

  if (!MusicPlayer.supported()) return null;
  const silent = s.muted || !s.track;
  return (
    <div className="flex items-center gap-1.5 rounded-full border-2 border-ink bg-white py-0.5 pl-1 pr-2 shadow-[0_2px_0_var(--color-ink)]">
      <button
        type="button"
        aria-label={s.muted ? t("game.music.unmute") : t("game.music.mute")}
        aria-pressed={s.muted}
        disabled={!s.track}
        onClick={() => setS((v) => ({ ...v, muted: !v.muted }))}
        className="flex h-8 w-8 items-center justify-center rounded-full transition hover:bg-mint disabled:opacity-40"
      >
        <SpeakerIcon off={silent} />
      </button>
      <select
        aria-label={t("game.music.label")}
        value={s.track ?? ""}
        onChange={(e) => setS((v) => ({ ...v, track: e.target.value || null, muted: false }))}
        className="max-w-36 cursor-pointer rounded-md bg-transparent py-1 text-sm font-bold text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-sea"
      >
        {TRACKS.map((tr) => (
          <option key={tr.id} value={tr.id}>
            ♪ {t(`game.music.tracks.${tr.id}`)}
          </option>
        ))}
        <option value="">{t("game.music.off")}</option>
      </select>
      <input
        type="range"
        min={0}
        max={1}
        step={0.05}
        aria-label={t("game.music.volume")}
        value={s.volume}
        disabled={!s.track}
        onChange={(e) => setS((v) => ({ ...v, volume: Number(e.target.value), muted: false }))}
        className="hidden w-20 accent-sea sm:block"
      />
    </div>
  );
}

function SpeakerIcon({ off }: { off: boolean }) {
  return (
    <svg aria-hidden viewBox="0 0 24 24" className="h-5 w-5" fill="none" stroke="currentColor" strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 9h3l5-4v14l-5-4H4z" fill="currentColor" />
      {off ? <path d="M16 9l5 6M21 9l-5 6" /> : <path d="M16 8.5a5 5 0 0 1 0 7M18.5 6a8.5 8.5 0 0 1 0 12" />}
    </svg>
  );
}
