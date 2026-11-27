// The host's "open in new window" view: a second browser window (for a
// second screen or a bigger projector window) that always shows the media
// of the question on the host screen. The two windows talk over a
// BroadcastChannel, which only reaches windows of the same site in the
// same browser; nothing goes through the server.

import { useEffect, useRef } from "react";
import type { Media } from "./QuestionMedia";

export type MediaFrame = Media & { text: string; index: number; total: number };

type Msg = { kind: "hello" } | { kind: "show"; frame: MediaFrame | null };

const channelName = (pin: string) => `myquiz.media.${pin}`;

export function mediaWindowUrl(pin: string) {
  return `/host/${encodeURIComponent(pin)}/media`;
}

/** Opens (or brings back) the pop-out window for this game. */
export function openMediaWindow(pin: string) {
  window.open(mediaWindowUrl(pin), `myquiz-media-${pin}`, "popup,width=1280,height=800");
}

/** Host side: keeps any open pop-out window showing `frame`. */
export function useMediaBroadcast(pin: string, frame: MediaFrame | null) {
  const latest = useRef(frame);
  latest.current = frame;
  const channel = useRef<BroadcastChannel | null>(null);
  useEffect(() => {
    if (typeof BroadcastChannel === "undefined") return;
    const ch = new BroadcastChannel(channelName(pin));
    channel.current = ch;
    // A window that opens later asks for what is on screen now.
    ch.onmessage = (e: MessageEvent<Msg>) => {
      if (e.data?.kind === "hello") ch.postMessage({ kind: "show", frame: latest.current } satisfies Msg);
    };
    return () => {
      ch.close();
      channel.current = null;
    };
  }, [pin]);
  const key = frame ? `${frame.index}|${frame.media}|${frame.markdown}` : "";
  useEffect(() => {
    channel.current?.postMessage({ kind: "show", frame: latest.current } satisfies Msg);
  }, [key]);
}

/** Pop-out side: what the host screen shows; `undefined` until it answers. */
export function subscribeMedia(pin: string, onFrame: (f: MediaFrame | null) => void): () => void {
  if (typeof BroadcastChannel === "undefined") return () => {};
  const ch = new BroadcastChannel(channelName(pin));
  ch.onmessage = (e: MessageEvent<Msg>) => {
    if (e.data?.kind === "show") onFrame(e.data.frame);
  };
  ch.postMessage({ kind: "hello" } satisfies Msg);
  return () => ch.close();
}
