// The host's controls for one player: mute their chat, remove them (they
// may join again under another name) or ban their phone from the game.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ClientMessage } from "../generated/ClientMessage";
import type { LobbyPlayer } from "../generated/LobbyPlayer";
import { Avatar } from "./avatars";
import { panel } from "./ui";

type Act = (msg: ClientMessage) => unknown;

const small = "inline-flex items-center justify-center gap-1 whitespace-nowrap rounded-xl border-2 border-ink px-2.5 py-1.5 text-sm font-bold transition active:translate-y-px";

/** Mute / Remove / Ban for one player. Ban asks once more. */
export function PlayerActions({ player, act, onDone }: { player: LobbyPlayer; act: Act; onDone?: () => void }) {
  const { t } = useTranslation();
  const [confirmBan, setConfirmBan] = useState(false);
  const name = player.nickname;
  const run = (msg: ClientMessage) => {
    void act(msg);
    onDone?.();
  };

  if (confirmBan) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-semibold">{t("game.players.banConfirm", { name })}</span>
        <button type="button" className={`${small} bg-coral text-white`} onClick={() => run({ t: "host:ban", d: { playerId: player.id } })}>
          {t("game.players.ban")}
        </button>
        <button type="button" className={`${small} border-transparent text-ink/60 hover:bg-ink/5`} onClick={() => setConfirmBan(false)}>
          {t("game.cancel")}
        </button>
      </div>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2">
      <button
        type="button"
        aria-label={t(player.muted ? "game.players.unmuteName" : "game.players.muteName", { name })}
        className={`${small} bg-white hover:bg-mint`}
        onClick={() => run({ t: "host:mute", d: { playerId: player.id, muted: !player.muted } })}
      >
        <span aria-hidden>{player.muted ? "🔊" : "🔇"}</span>
        {t(player.muted ? "game.players.unmute" : "game.players.mute")}
      </button>
      <button
        type="button"
        aria-label={t("game.kick", { name })}
        title={t("game.players.removeHint")}
        className={`${small} bg-white hover:bg-sand`}
        onClick={() => run({ t: "host:kick", d: { playerId: player.id } })}
      >
        {t("game.players.remove")}
      </button>
      <button
        type="button"
        aria-label={t("game.players.banName", { name })}
        title={t("game.players.banHint")}
        className={`${small} bg-coral/15 text-red-800 hover:bg-coral hover:text-white`}
        onClick={() => setConfirmBan(true)}
      >
        {t("game.players.ban")}
      </button>
    </div>
  );
}

function MutedMark() {
  const { t } = useTranslation();
  return (
    <span role="img" aria-label={t("game.players.muted")} title={t("game.players.muted")} className="text-base">
      🔇
    </span>
  );
}

/** A player's chip in the lobby, with a menu of the host's controls. */
export function LobbyPlayerChip({ player, act }: { player: LobbyPlayer; act: Act }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLLIElement>(null);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <li ref={root} className="relative flex animate-pop items-center gap-2 rounded-full border-2 border-ink bg-white py-1 pl-1 pr-1 text-lg font-bold">
      <Avatar id={player.avatar} size={40} />
      {player.nickname}
      {player.muted && <MutedMark />}
      <button
        type="button"
        aria-label={t("game.players.menuFor", { name: player.nickname })}
        aria-expanded={open}
        className="ml-1 flex h-7 w-7 items-center justify-center rounded-full text-base text-ink/50 transition hover:bg-ink/10 hover:text-ink focus-visible:bg-ink/10"
        onClick={() => setOpen((o) => !o)}
      >
        ⋯
      </button>
      {open && (
        <div className={`${panel} absolute left-0 top-full z-20 mt-2 w-max max-w-[min(22rem,80vw)] bg-white p-2 text-base`}>
          <PlayerActions player={player} act={act} onDone={() => setOpen(false)} />
        </div>
      )}
    </li>
  );
}

/** During the game: every player, to mute, remove or ban any of them. */
export function PlayersDialog({ players, act, onClose }: { players: LobbyPlayer[]; act: Act; onClose: () => void }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");

  useEffect(() => {
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", esc);
    return () => document.removeEventListener("keydown", esc);
  }, [onClose]);

  const q = query.trim().toLowerCase();
  const shown = q ? players.filter((p) => p.nickname.toLowerCase().includes(q)) : players;
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-ink/40 p-4" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-label={t("game.players.title")} className={`${panel} flex max-h-[85vh] w-full max-w-xl flex-col gap-3 bg-white p-5`}>
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-xl font-extrabold">{t("game.playerCount", { count: players.length })}</h2>
          <button type="button" className="rounded-xl px-3 py-1.5 text-sm font-bold text-ink/60 hover:bg-ink/5" onClick={onClose}>
            {t("game.players.close")}
          </button>
        </div>
        {players.length > 6 && (
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("game.players.search")}
            aria-label={t("game.players.search")}
            className="rounded-xl border-2 border-ink px-3 py-2 text-base focus:outline-none focus:ring-4 focus:ring-lagoon/50"
          />
        )}
        <ul className="flex flex-col divide-y-2 divide-ink/10 overflow-y-auto">
          {shown.map((p) => (
            <li key={p.id} className="flex flex-wrap items-center gap-x-3 gap-y-2 py-2">
              <Avatar id={p.avatar} size={36} />
              <span className="min-w-0 flex-1 truncate text-lg font-bold">{p.nickname}</span>
              {p.muted && <MutedMark />}
              <PlayerActions player={p} act={act} />
            </li>
          ))}
          {shown.length === 0 && <li className="py-4 text-center text-ink/60">{t("game.players.none")}</li>}
        </ul>
      </div>
    </div>
  );
}
