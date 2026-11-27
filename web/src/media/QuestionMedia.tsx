import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import "highlight.js/styles/github.css";
import { renderMarkdown } from "./markdown";

/** What a question shows besides its text: an uploaded image, Markdown
 *  (usually code), or both. */
export type Media = { media: string | null; markdown: string | null };

export function hasMedia(m: Media | null | undefined): m is Media {
  return !!m && (!!m.media || !!m.markdown?.trim());
}

/**
 * The font size (px) at which `md` fits `w` x `h`, from its size now. Code
 * blocks scroll sideways rather than wrap, so their longest line counts;
 * other text wraps, so only its height does.
 */
export function fitFontPx(md: HTMLElement, w: number, h: number, min: number, max: number): number {
  const cur = parseFloat(getComputedStyle(md).fontSize) || 16;
  // A block is as wide as its box; the code's own width is its longest line.
  const pres = [...md.querySelectorAll("pre")].map((p) => {
    const old = p.style.width;
    p.style.width = "max-content";
    const width = p.getBoundingClientRect().width;
    p.style.width = old;
    return width;
  });
  const natH = Math.max(md.scrollHeight, 1);
  const byWidth = pres.length ? w / Math.max(...pres, 1) : Infinity;
  const px = cur * Math.min(byWidth, h / natH);
  return Math.min(max, Math.max(min, Math.floor(px * 10) / 10));
}

/** Sanitised Markdown. `className` sets the size. */
export function Markdown({ source, className = "", style }: { source: string; className?: string; style?: React.CSSProperties }) {
  const html = useMemo(() => renderMarkdown(source), [source]);
  return <div className={`md text-left ${className}`} style={style} dangerouslySetInnerHTML={{ __html: html }} />;
}

/**
 * The media area of a question, at the size the screen gives it, with an
 * "enlarge" button. Clicking the media itself also enlarges it.
 */
export function QuestionMedia({
  value,
  className = "",
  maxClass = "max-h-[40vh]",
  codeClass = "md-stage",
  fit = [14, 34],
  onZoom,
}: {
  value: Media;
  className?: string;
  /** Height limit for the image and the code box, e.g. max-h-[40vh]. */
  maxClass?: string;
  /** Starting font size class for Markdown: md-stage (projector), md-phone. */
  codeClass?: string;
  /** Smallest and largest font size (px) when fitting code to the box. */
  fit?: [number, number];
  onZoom?: () => void;
}) {
  const { t } = useTranslation();
  const zoomable = onZoom ? "cursor-zoom-in" : "";
  const box = useRef<HTMLDivElement>(null);
  const [px, setPx] = useState<number | null>(null);
  const [min, max] = fit;
  // Code shrinks (or grows) so the whole program is on screen; below the
  // smallest size it scrolls, and the enlarge button shows it all.
  useLayoutEffect(() => {
    const b = box.current;
    if (!b) return;
    const measure = () => {
      const md = b.querySelector<HTMLElement>(".md");
      if (!md) return;
      const limit = parseFloat(getComputedStyle(b).maxHeight);
      const h = (Number.isFinite(limit) ? limit : b.clientHeight) - 30;
      setPx(fitFontPx(md, b.clientWidth - 38, h, min, max));
    };
    // Measured again once the first size is applied and whenever the box
    // changes size (layout settling, window resize), so it converges.
    measure();
    const again = requestAnimationFrame(measure);
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => measure());
    ro?.observe(b);
    window.addEventListener("resize", measure);
    return () => {
      cancelAnimationFrame(again);
      ro?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [value.markdown, min, max]);
  return (
    <div className={`relative flex min-w-0 flex-col items-center gap-3 ${className}`} data-testid="question-media">
      {value.media && (
        <img src={value.media} alt="" onClick={onZoom} className={`min-w-0 max-w-full rounded-2xl border-[3px] border-ink bg-white object-contain ${maxClass} ${zoomable}`} />
      )}
      {value.markdown?.trim() && (
        <div ref={box} onClick={onZoom} className={`w-full overflow-auto rounded-2xl border-[3px] border-ink bg-white px-4 py-3 ${maxClass} ${zoomable}`}>
          <Markdown source={value.markdown} className={codeClass} style={px ? { fontSize: `${px}px` } : undefined} />
        </div>
      )}
      {onZoom && (
        <button
          type="button"
          onClick={onZoom}
          aria-label={t("media.enlarge")}
          title={t("media.enlargeHint")}
          className="absolute right-2 top-2 flex h-10 w-10 items-center justify-center rounded-full border-2 border-ink bg-white/90 text-lg shadow-[0_2px_0_var(--color-ink)] hover:bg-mint"
        >
          <ZoomIcon />
        </button>
      )}
    </div>
  );
}

const MIN_ZOOM = 0.5;
const MAX_ZOOM = 5;
const clampZoom = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(z * 100) / 100));

/**
 * The media as large as the box allows, with zoom: + and - buttons and
 * keys, 0 to fit again, pinch on touch screens, Ctrl + wheel on a laptop.
 * Code starts at the font size that makes the whole block fit.
 */
export function ZoomView({ value, keys = true, toolbar }: { value: Media; keys?: boolean; toolbar?: ReactNode }) {
  const { t } = useTranslation();
  const [zoom, setZoom] = useState(1);
  const box = useRef<HTMLDivElement>(null);
  const code = useRef<HTMLDivElement>(null);
  const [fitPx, setFitPx] = useState(24);
  const [boxH, setBoxH] = useState(600);
  const pinch = useRef<{ dist: number; zoom: number } | null>(null);

  // Reset when the content changes (a new question in the pop-out window).
  useEffect(() => setZoom(1), [value.media, value.markdown]);

  // The font size at which the whole code block fits the box.
  const fit = useCallback(() => {
    const b = box.current;
    if (!b) return;
    setBoxH(b.clientHeight);
    const md = code.current?.querySelector<HTMLElement>(".md");
    if (!md) return;
    const room = value.media ? 0.45 : 1; // an image shares the box
    // 48: this box's padding; 48 and 32: the white card's padding.
    setFitPx(fitFontPx(md, b.clientWidth - 48 - 48, (b.clientHeight - 48) * room - 32, 12, 64));
  }, [value.media]);
  useLayoutEffect(() => {
    fit();
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [fit, value.markdown]);

  useEffect(() => {
    if (!keys) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.key === "+" || e.key === "=") setZoom((z) => clampZoom(z * 1.25));
      else if (e.key === "-" || e.key === "_") setZoom((z) => clampZoom(z / 1.25));
      else if (e.key === "0") setZoom(1);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [keys]);

  // Ctrl + wheel (and a trackpad pinch, which browsers send as one) zooms
  // the media instead of the whole page. Needs a non-passive listener.
  useEffect(() => {
    const b = box.current;
    if (!b) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      setZoom((z) => clampZoom(z * Math.exp(-e.deltaY / 200)));
    };
    b.addEventListener("wheel", onWheel, { passive: false });
    return () => b.removeEventListener("wheel", onWheel);
  }, []);

  const dist = (e: React.TouchEvent) => {
    const [a, b] = [e.touches[0], e.touches[1]];
    return Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
  };

  const pct = Math.round(zoom * 100);
  return (
    <div className="flex h-full min-h-0 w-full flex-col">
      <div
        ref={box}
        data-testid="zoom-box"
        // One finger scrolls; two fingers zoom the media, not the page.
        style={{ touchAction: "pan-x pan-y" }}
        className="relative min-h-0 flex-1 overflow-auto"
        onTouchStart={(e) => {
          if (e.touches.length === 2) pinch.current = { dist: dist(e), zoom };
        }}
        onTouchMove={(e) => {
          if (e.touches.length === 2 && pinch.current) setZoom(clampZoom((pinch.current.zoom * dist(e)) / pinch.current.dist));
        }}
        onTouchEnd={(e) => {
          if (e.touches.length < 2) pinch.current = null;
        }}
      >
        <div
          // Zoomed in, the content grows from the top left so every part of
          // it can be scrolled to.
          className={`flex min-h-full flex-col gap-6 p-6 ${zoom <= 1 ? "items-center justify-center" : "w-max min-w-full items-start"}`}
          style={{ width: zoom > 1 && value.media && !value.markdown?.trim() ? `${zoom * 100}%` : undefined }}
        >
          {value.media && (
            <img
              src={value.media}
              alt=""
              className="rounded-xl bg-white object-contain"
              style={
                zoom > 1
                  ? { width: "100%", maxWidth: "none" }
                  : { maxWidth: `${zoom * 100}%`, maxHeight: Math.max(120, (boxH - 48) * (value.markdown?.trim() ? 0.45 : 1) * zoom), height: "auto" }
              }
            />
          )}
          {value.markdown?.trim() && (
            <div ref={code} className="max-w-full rounded-xl bg-white px-6 py-4" style={zoom > 1 ? { maxWidth: "none" } : undefined}>
              <Markdown source={value.markdown} style={{ fontSize: `${fitPx * zoom}px` }} />
            </div>
          )}
        </div>
      </div>
      <div className="flex shrink-0 flex-wrap items-center justify-center gap-2 p-3">
        <button type="button" className={toolBtn} onClick={() => setZoom((z) => clampZoom(z / 1.25))} aria-label={t("media.zoomOut")} title={t("media.zoomOut")}>
          −
        </button>
        <button type="button" className={`${toolBtn} w-20 tabular-nums`} onClick={() => setZoom(1)} aria-label={t("media.fit")} title={t("media.fit")}>
          {pct}%
        </button>
        <button type="button" className={toolBtn} onClick={() => setZoom((z) => clampZoom(z * 1.25))} aria-label={t("media.zoomIn")} title={t("media.zoomIn")}>
          +
        </button>
        {toolbar}
      </div>
    </div>
  );
}

const toolBtn =
  "inline-flex h-11 min-w-11 items-center justify-center rounded-xl border-2 border-ink bg-white px-3 text-xl font-black text-ink shadow-[0_2px_0_var(--color-ink)] hover:bg-mint";

/** The media full screen, over everything. Esc or the close button closes. */
export function MediaZoom({ value, onClose, onPopOut }: { value: Media; onClose: () => void; onPopOut?: () => void }) {
  const { t } = useTranslation();
  const closeRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return createPortal(
    <div role="dialog" aria-modal="true" aria-label={t("media.enlarged")} className="fixed inset-0 z-50 flex flex-col bg-ink/90 text-ink">
      <ZoomView
        value={value}
        toolbar={
          <>
            {onPopOut && (
              <button type="button" className={`${toolBtn} text-base`} onClick={onPopOut}>
                {t("media.popOut")} ↗
              </button>
            )}
            <button ref={closeRef} type="button" className={`${toolBtn} text-base`} onClick={onClose}>
              {t("media.close")}
            </button>
          </>
        }
      />
    </div>,
    document.body,
  );
}

function ZoomIcon() {
  return (
    <svg aria-hidden viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" className="h-5 w-5">
      <circle cx="8.5" cy="8.5" r="5.5" />
      <path d="M12.5 12.5 17 17M8.5 6v5M6 8.5h5" />
    </svg>
  );
}
