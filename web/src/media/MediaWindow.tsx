import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { hasMedia, ZoomView } from "./QuestionMedia";
import { subscribeMedia, type MediaFrame } from "./popout";

/**
 * `/host/:pin/media`: the pop-out window. It follows the host screen, so
 * the host can drag it to a second screen or make it as big as needed.
 */
export function MediaWindow() {
  const { pin = "" } = useParams();
  const { t } = useTranslation();
  const [frame, setFrame] = useState<MediaFrame | null | undefined>(undefined);
  useEffect(() => subscribeMedia(pin, setFrame), [pin]);
  useEffect(() => {
    document.title = frame ? `${t("game.questionOf", { n: frame.index + 1, total: frame.total })} · ${t("media.windowTitle")}` : t("media.windowTitle");
  }, [frame, t]);

  return (
    <div className="flex h-dvh flex-col bg-paper text-ink">
      {frame && (
        <h1 className="shrink-0 border-b-[3px] border-ink/10 px-6 py-3 text-xl font-extrabold sm:text-2xl">
          <span className="mr-3 text-ink/50">{frame.index + 1}/{frame.total}</span>
          {frame.text}
        </h1>
      )}
      <div className="min-h-0 flex-1">
        {frame && hasMedia(frame) ? (
          <ZoomView value={frame} />
        ) : (
          <p className="flex h-full items-center justify-center p-8 text-center text-lg font-semibold text-ink/60">
            {frame === undefined ? t("media.windowConnecting") : t("media.windowEmpty")}
          </p>
        )}
      </div>
    </div>
  );
}
