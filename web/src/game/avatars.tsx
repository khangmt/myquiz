// The players' avatars, drawn with the game's ink outline in two looks the
// host picks from: "cute" round animal faces and "anime" chibi kids, all
// original. The ids must match `AVATARS` in crates/protocol/src/ws.rs; each
// look draws its own character for an id, so a player keeps their slot when
// the host switches looks.
import { createContext, useContext } from "react";

export type Theme = "cute" | "anime";
export const THEMES: Theme[] = ["cute", "anime"];

/** The game's look, for every avatar on screen. */
export const ThemeContext = createContext<Theme>("cute");

export function asTheme(t: string | null | undefined): Theme {
  return t === "anime" ? "anime" : "cute";
}

const INK = "#3b3a36";
const BLUSH = "#f4a7a8";

type Ears = "long" | "pointy" | "round" | "big" | "bumps" | "tuft" | "floppy" | "none";
type Snout = "smile" | "beak" | "pig" | "bear" | "koala" | "whiskers";

type Spec = { face: string; bg: string; ears: Ears; earColor?: string; inner?: string; snout: Snout; patches?: boolean };

const SPECS: Record<string, Spec> = {
  bunny: { face: "#ffffff", bg: "#e3f1d6", ears: "long", inner: BLUSH, snout: "smile" },
  kitty: { face: "#f6c58f", bg: "#fbe0e0", ears: "pointy", inner: BLUSH, snout: "whiskers" },
  froggy: { face: "#9ccf8a", bg: "#c9f0e6", ears: "bumps", snout: "smile" },
  ducky: { face: "#f7d66a", bg: "#cfe9f2", ears: "tuft", snout: "beak" },
  bear: { face: "#c9996b", bg: "#f2eb9f", ears: "round", inner: "#e8c9a6", snout: "bear" },
  fox: { face: "#ec9a5b", bg: "#e3f1d6", ears: "pointy", inner: "#fff4e6", snout: "smile" },
  panda: { face: "#ffffff", bg: "#d5ecc6", ears: "round", earColor: INK, snout: "smile", patches: true },
  chick: { face: "#fbe38a", bg: "#fbe0e0", ears: "tuft", snout: "beak" },
  piggy: { face: "#f7c1c4", bg: "#e3f1d6", ears: "floppy", snout: "pig" },
  koala: { face: "#b9bfc4", bg: "#f2eb9f", ears: "big", inner: "#e9ecee", snout: "koala" },
  mouse: { face: "#d8d4cf", bg: "#c9f0e6", ears: "big", inner: BLUSH, snout: "whiskers" },
  seal: { face: "#c9dbe3", bg: "#fbf3d4", ears: "none", snout: "whiskers" },
};

export const AVATAR_IDS = Object.keys(SPECS);

export function randomAvatar(): string {
  return AVATAR_IDS[Math.floor(Math.random() * AVATAR_IDS.length)];
}

const line = { stroke: INK, strokeWidth: 3, strokeLinejoin: "round" as const, strokeLinecap: "round" as const };

function EarShapes({ s }: { s: Spec }) {
  const fill = s.earColor ?? s.face;
  switch (s.ears) {
    case "long":
      return (
        <>
          {[38, 62].map((x) => (
            <g key={x}>
              <ellipse cx={x} cy={22} rx={8} ry={18} fill={fill} {...line} />
              <ellipse cx={x} cy={24} rx={3.5} ry={11} fill={s.inner} />
            </g>
          ))}
        </>
      );
    case "pointy":
      return (
        <>
          <path d="M24 46 L28 16 L48 30 Z" fill={fill} {...line} />
          <path d="M76 46 L72 16 L52 30 Z" fill={fill} {...line} />
          <path d="M30 36 L31 24 L40 31 Z" fill={s.inner} />
          <path d="M70 36 L69 24 L60 31 Z" fill={s.inner} />
        </>
      );
    case "round":
    case "big": {
      const r = s.ears === "big" ? 14 : 10;
      const y = s.ears === "big" ? 34 : 30;
      return (
        <>
          {[26, 74].map((x) => (
            <g key={x}>
              <circle cx={x} cy={y} r={r} fill={fill} {...line} />
              {s.inner && <circle cx={x} cy={y + 1} r={r - 6} fill={s.inner} />}
            </g>
          ))}
        </>
      );
    }
    case "bumps":
      return (
        <>
          <circle cx={36} cy={32} r={11} fill={fill} {...line} />
          <circle cx={64} cy={32} r={11} fill={fill} {...line} />
        </>
      );
    case "tuft":
      return <path d="M50 28 C46 20 48 14 53 14 C51 18 54 22 57 21 C55 25 53 27 50 28 Z" fill={s.face} {...line} />;
    case "floppy":
      return (
        <>
          <path d="M26 40 L30 22 L44 32 Z" fill="#eaa2a7" {...line} />
          <path d="M74 40 L70 22 L56 32 Z" fill="#eaa2a7" {...line} />
        </>
      );
    default:
      return null;
  }
}

function Face({ s }: { s: Spec }) {
  // Frogs look out of the bumps on top of their head.
  const eyeY = s.ears === "bumps" ? 33 : 54;
  const eyes = [s.ears === "bumps" ? 36 : 40, s.ears === "bumps" ? 64 : 60];
  return (
    <>
      {s.patches &&
        eyes.map((x, i) => <ellipse key={x} cx={x} cy={eyeY} rx={6} ry={7.5} fill={INK} transform={`rotate(${i ? 25 : -25} ${x} ${eyeY})`} />)}
      {eyes.map((x) => (
        <g key={x}>
          <ellipse cx={x} cy={eyeY} rx={3.4} ry={4.2} fill={s.patches ? "#ffffff" : INK} />
          <circle cx={x + 1} cy={eyeY - 1.6} r={1.2} fill={s.patches ? INK : "#ffffff"} />
        </g>
      ))}
      <ellipse cx={33} cy={64} rx={5} ry={3} fill={BLUSH} opacity={0.85} />
      <ellipse cx={67} cy={64} rx={5} ry={3} fill={BLUSH} opacity={0.85} />
      {s.snout === "smile" && <path d="M45 63 C47 67 53 67 55 63" fill="none" {...line} strokeWidth={2.5} />}
      {s.snout === "beak" && <ellipse cx={50} cy={64} rx={7} ry={4} fill="#ef9a4a" {...line} strokeWidth={2.5} />}
      {s.snout === "pig" && (
        <>
          <ellipse cx={50} cy={64} rx={8} ry={6} fill="#eaa2a7" {...line} strokeWidth={2.5} />
          <circle cx={47} cy={64} r={1.5} fill={INK} />
          <circle cx={53} cy={64} r={1.5} fill={INK} />
        </>
      )}
      {s.snout === "bear" && (
        <>
          <ellipse cx={50} cy={65} rx={9} ry={7} fill="#ecd3b6" />
          <ellipse cx={50} cy={62} rx={3.5} ry={2.5} fill={INK} />
          <path d="M47 67 C49 69 51 69 53 67" fill="none" {...line} strokeWidth={2} />
        </>
      )}
      {s.snout === "koala" && <ellipse cx={50} cy={62} rx={5} ry={7} fill={INK} />}
      {s.snout === "whiskers" && (
        <>
          <ellipse cx={50} cy={61} rx={2.5} ry={1.8} fill={INK} />
          <path d="M46 65 C48 67 50 66 50 64 C50 66 52 67 54 65" fill="none" {...line} strokeWidth={2} />
          <path d="M22 60 L32 61 M22 66 L32 65 M78 60 L68 61 M78 66 L68 65" {...line} strokeWidth={1.8} />
        </>
      )}
    </>
  );
}

/** A player's avatar, in the game's look. Unknown ids fall back to the first. */
export function Avatar({ id, size = 40, className = "", theme }: { id?: string | null; size?: number; className?: string; theme?: Theme }) {
  const current = useContext(ThemeContext);
  if ((theme ?? current) === "anime") return <AnimeAvatar id={id} size={size} className={className} />;
  const s = SPECS[id ?? ""] ?? SPECS.bunny;
  return (
    <svg aria-hidden viewBox="0 0 100 100" width={size} height={size} className={`shrink-0 ${className}`}>
      <circle cx={50} cy={50} r={47} fill={s.bg} {...line} />
      <EarShapes s={s} />
      <circle cx={50} cy={58} r={29} fill={s.face} {...line} />
      <Face s={s} />
    </svg>
  );
}

/** The grid a player picks their avatar from when joining. */
export function AvatarPicker({ value, onChange, label, theme }: { value: string; onChange: (id: string) => void; label: string; theme?: Theme }) {
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="mb-2 text-sm font-bold text-ink/70">{label}</legend>
      <div className="grid grid-cols-4 gap-2">
        {AVATAR_IDS.map((id) => (
          <label
            key={id}
            className={`flex cursor-pointer items-center justify-center rounded-2xl p-1 transition has-[:focus-visible]:ring-4 has-[:focus-visible]:ring-sea/40 ${
              value === id ? "bg-sun/60 ring-[3px] ring-ink" : "hover:bg-ink/5"
            }`}
          >
            <input type="radio" name="avatar" value={id} checked={value === id} onChange={() => onChange(id)} className="sr-only" aria-label={id} />
            <Avatar id={id} theme={theme} size={56} className={value === id ? "animate-pop" : ""} />
          </label>
        ))}
      </div>
    </fieldset>
  );
}

// ---- Anime look ----------------------------------------------------------

type Hair = "bob" | "long" | "twintails" | "ponytail" | "bun" | "spiky" | "messy" | "short";
type Extra = "bow" | "catears" | "star" | "glasses" | "flower" | "headphones" | "leaf" | "none";
type Kid = { hair: string; style: Hair; eyes: string; skin: string; bg: string; extra: Extra; extraColor?: string };

const SKIN = ["#fde7d4", "#f6d3b5", "#e9b98f"];

const KIDS: Record<string, Kid> = {
  bunny: { hair: "#f4a7a8", style: "twintails", eyes: "#c4517f", skin: SKIN[0], bg: "#e3f1d6", extra: "bow", extraColor: "#d9544c" },
  kitty: { hair: "#3b3a36", style: "bob", eyes: "#2a7a90", skin: SKIN[1], bg: "#fbe0e0", extra: "catears" },
  froggy: { hair: "#7bb474", style: "messy", eyes: "#4d8747", skin: SKIN[0], bg: "#c9f0e6", extra: "headphones", extraColor: "#2a7a90" },
  ducky: { hair: "#f2c14e", style: "short", eyes: "#2a7a90", skin: SKIN[0], bg: "#cfe9f2", extra: "star", extraColor: "#d9544c" },
  bear: { hair: "#8a6a4a", style: "ponytail", eyes: "#8a6a4a", skin: SKIN[2], bg: "#f2eb9f", extra: "flower", extraColor: "#f4a7a8" },
  fox: { hair: "#ec8a4b", style: "spiky", eyes: "#d9544c", skin: SKIN[1], bg: "#e3f1d6", extra: "none" },
  panda: { hair: "#e9ecee", style: "long", eyes: "#3b3a36", skin: SKIN[0], bg: "#d5ecc6", extra: "glasses" },
  chick: { hair: "#fbe38a", style: "bun", eyes: "#ec8a4b", skin: SKIN[0], bg: "#fbe0e0", extra: "leaf", extraColor: "#7bb474" },
  piggy: { hair: "#c4517f", style: "long", eyes: "#c4517f", skin: SKIN[1], bg: "#e3f1d6", extra: "bow", extraColor: "#f2c14e" },
  koala: { hair: "#5f7d8c", style: "messy", eyes: "#2a7a90", skin: SKIN[2], bg: "#f2eb9f", extra: "glasses" },
  mouse: { hair: "#7cdeca", style: "twintails", eyes: "#2a7a90", skin: SKIN[0], bg: "#fbf3d4", extra: "star", extraColor: "#f2c14e" },
  seal: { hair: "#2a7a90", style: "spiky", eyes: "#2a7a90", skin: SKIN[1], bg: "#fbf3d4", extra: "headphones", extraColor: "#d9544c" },
};

const FACE = "M29 54 C29 74 40 85 50 85 C60 85 71 74 71 54 C71 40 62 33 50 33 C38 33 29 40 29 54 Z";
const BANGS = "M25 56 C22 33 35 21 50 21 C65 21 78 33 75 56 C71 49 67 45 64 40 C61 46 56 47 53 41 C49 47 43 49 41 41 C37 47 32 51 25 56 Z";

function BackHair({ k }: { k: Kid }) {
  const f = { fill: k.hair, ...line };
  const bob = <path d="M22 70 C17 36 32 19 50 19 C68 19 83 36 78 70 C70 74 30 74 22 70 Z" {...f} />;
  switch (k.style) {
    case "long":
      return <path d="M20 90 C14 44 28 18 50 18 C72 18 86 44 80 90 Z" {...f} />;
    case "twintails":
      return (
        <>
          <path d="M22 44 C8 52 6 74 14 86 C18 74 22 62 26 54 Z" {...f} />
          <path d="M78 44 C92 52 94 74 86 86 C82 74 78 62 74 54 Z" {...f} />
          {bob}
        </>
      );
    case "ponytail":
      return (
        <>
          <path d="M70 26 C90 24 94 50 84 70 C82 56 78 44 70 36 Z" {...f} />
          {bob}
        </>
      );
    case "bun":
      return (
        <>
          <circle cx={50} cy={16} r={10} {...f} />
          {bob}
        </>
      );
    case "spiky":
      return <path d="M22 62 L16 44 L27 42 L22 27 L37 30 L38 14 L50 25 L62 12 L64 29 L79 26 L74 41 L85 44 L78 62 Z" {...f} />;
    case "messy":
      return <path d="M22 64 C14 54 18 44 22 40 C18 28 30 18 40 20 C46 12 58 12 64 20 C76 18 84 30 78 40 C84 46 86 56 78 64 Z" {...f} />;
    case "short":
      return <path d="M25 60 C20 36 34 21 50 21 C66 21 80 36 75 60 Z" {...f} />;
    default:
      return bob;
  }
}

function AnimeEye({ x, color }: { x: number; color: string }) {
  const y = 59;
  return (
    <g>
      <ellipse cx={x} cy={y} rx={6.2} ry={7.8} fill={color} stroke={INK} strokeWidth={2} />
      <ellipse cx={x} cy={y + 1.4} rx={3.1} ry={4.4} fill={INK} />
      <circle cx={x - 2.2} cy={y - 3} r={2.4} fill="#ffffff" />
      <circle cx={x + 2} cy={y + 3} r={0.9} fill="#ffffff" />
      <path d={`M${x - 7.5} ${y - 5} C${x - 3.5} ${y - 10} ${x + 3.5} ${y - 10} ${x + 7.5} ${y - 5}`} fill="none" stroke={INK} strokeWidth={2.6} strokeLinecap="round" />
    </g>
  );
}

function KidExtra({ k }: { k: Kid }) {
  const c = k.extraColor ?? INK;
  switch (k.extra) {
    case "bow":
      return (
        <g transform="translate(70 26) rotate(20)">
          <path d="M0 0 L-10 -6 L-10 6 Z M0 0 L10 -6 L10 6 Z" fill={c} {...line} strokeWidth={2.2} />
          <circle r={2.6} fill={c} {...line} strokeWidth={2} />
        </g>
      );
    case "catears":
      return (
        <>
          <path d="M26 34 L28 12 L42 24 Z" fill={k.hair} {...line} />
          <path d="M74 34 L72 12 L58 24 Z" fill={k.hair} {...line} />
          <path d="M30 28 L31 18 L37 24 Z" fill={BLUSH} />
          <path d="M70 28 L69 18 L63 24 Z" fill={BLUSH} />
        </>
      );
    case "star":
      return <path d="M72 22 L74.5 28 L81 28.5 L76 32.5 L77.5 39 L72 35.5 L66.5 39 L68 32.5 L63 28.5 L69.5 28 Z" fill={c} {...line} strokeWidth={2} />;
    case "glasses":
      return (
        <g fill="none" stroke={INK} strokeWidth={2}>
          <circle cx={39} cy={59} r={9.5} />
          <circle cx={61} cy={59} r={9.5} />
          <path d="M48.5 58 L51.5 58" />
        </g>
      );
    case "flower":
      return (
        <g transform="translate(29 30)">
          {[0, 72, 144, 216, 288].map((a) => (
            <ellipse key={a} cx={0} cy={-5} rx={3.4} ry={5} fill={c} stroke={INK} strokeWidth={1.6} transform={`rotate(${a})`} />
          ))}
          <circle r={2.6} fill="#f2c14e" stroke={INK} strokeWidth={1.6} />
        </g>
      );
    case "headphones":
      return (
        <>
          <path d="M22 54 C20 26 80 26 78 54" fill="none" stroke={INK} strokeWidth={4} />
          <rect x={16} y={50} width={10} height={16} rx={5} fill={c} {...line} strokeWidth={2.2} />
          <rect x={74} y={50} width={10} height={16} rx={5} fill={c} {...line} strokeWidth={2.2} />
        </>
      );
    case "leaf":
      return <path d="M66 24 C74 14 86 16 86 16 C86 16 84 28 74 32 C70 33 67 30 66 24 Z" fill={c} {...line} strokeWidth={2} />;
    default:
      return null;
  }
}

function AnimeAvatar({ id, size, className }: { id?: string | null; size: number; className: string }) {
  const k = KIDS[id ?? ""] ?? KIDS.bunny;
  return (
    <svg aria-hidden viewBox="0 0 100 100" width={size} height={size} className={`shrink-0 ${className}`}>
      <defs>
        <clipPath id="anime-clip">
          <circle cx={50} cy={50} r={46} />
        </clipPath>
      </defs>
      <circle cx={50} cy={50} r={47} fill={k.bg} {...line} />
      <g clipPath="url(#anime-clip)">
        <BackHair k={k} />
        <path d="M42 80 L42 90 L58 90 L58 80 Z" fill={k.skin} {...line} strokeWidth={2.4} />
        <path d={FACE} fill={k.skin} {...line} />
        <path d={BANGS} fill={k.hair} {...line} />
        <AnimeEye x={39} color={k.eyes} />
        <AnimeEye x={61} color={k.eyes} />
        <path d="M31 68 L33 65 M34 69 L36 66 M64 69 L66 66 M67 68 L69 65" stroke={BLUSH} strokeWidth={2} strokeLinecap="round" />
        <path d="M47 73 C49 75.5 51 75.5 53 73" fill="none" stroke={INK} strokeWidth={2} strokeLinecap="round" />
        <KidExtra k={k} />
      </g>
      <circle cx={50} cy={50} r={47} fill="none" {...line} />
    </svg>
  );
}
