// The live-game WebSocket client (design section 7).
//
// Frames are { t, id?, d }. A request with an id gets { t: "ack", id, ok,
// d, code }; pushes have no id. The socket reconnects by itself with
// backoff; `onOpen` runs after every (re)connect so the screen can attach or
// resume again, and the server's snapshot then redraws it.

import type { Ack } from "../generated/Ack";
import type { ClientMessage } from "../generated/ClientMessage";
import type { ServerPush } from "../generated/ServerPush";
import type { TimePong } from "../generated/TimePong";

export class AckError extends Error {
  constructor(public code: string) {
    super(code);
  }
}

type Pending = { resolve: (d: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> };

export type SocketStatus = "connecting" | "open" | "reconnecting" | "closed";

export type SocketOptions = {
  url?: string;
  onPush: (push: ServerPush) => void;
  onOpen: () => void;
  onStatus?: (s: SocketStatus) => void;
  /** For tests. */
  create?: (url: string) => WebSocket;
};

const ACK_TIMEOUT_MS = 8000;

export function defaultUrl(): string {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}/ws`;
}

export class GameSocket {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private retries = 0;
  private closed = false;
  /** serverNow ≈ Date.now() + offset */
  offset = 0;

  constructor(private opts: SocketOptions) {
    this.connect();
  }

  private connect() {
    const url = this.opts.url ?? defaultUrl();
    this.opts.onStatus?.(this.retries === 0 ? "connecting" : "reconnecting");
    const ws = this.opts.create ? this.opts.create(url) : new WebSocket(url);
    this.ws = ws;
    ws.onopen = () => {
      this.retries = 0;
      this.opts.onStatus?.("open");
      this.opts.onOpen();
      void this.syncClock();
    };
    ws.onmessage = (e) => this.receive(String(e.data));
    ws.onclose = () => {
      for (const [, p] of this.pending) {
        clearTimeout(p.timer);
        p.reject(new AckError("DISCONNECTED"));
      }
      this.pending.clear();
      if (this.closed) {
        this.opts.onStatus?.("closed");
        return;
      }
      // 0.5 s, 1 s, 2 s ... up to 10 s, with jitter so 200 phones don't
      // reconnect in the same instant after a Wi-Fi blip.
      const delay = Math.min(10_000, 500 * 2 ** this.retries) * (0.75 + Math.random() / 2);
      this.retries += 1;
      this.opts.onStatus?.("reconnecting");
      setTimeout(() => this.connect(), delay);
    };
  }

  private receive(raw: string) {
    let msg: { t: string; id?: number };
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }
    if (msg.t === "ack") {
      const ack = msg as Ack;
      const p = this.pending.get(ack.id);
      if (!p) return;
      this.pending.delete(ack.id);
      clearTimeout(p.timer);
      if (ack.ok) p.resolve(ack.d);
      else p.reject(new AckError(ack.code ?? "INTERNAL"));
      return;
    }
    this.opts.onPush(msg as ServerPush);
  }

  /** Sends a request and resolves with the ack's `d`, or rejects with its code. */
  request<T>(msg: ClientMessage): Promise<T> {
    const ws = this.ws;
    if (!ws || ws.readyState !== WebSocket.OPEN) return Promise.reject(new AckError("DISCONNECTED"));
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new AckError("TIMEOUT"));
      }, ACK_TIMEOUT_MS);
      this.pending.set(id, { resolve: resolve as (d: unknown) => void, reject, timer });
      ws.send(JSON.stringify({ ...msg, id }));
    });
  }

  /** Estimates the server clock from three pings, keeping the fastest. */
  async syncClock() {
    let best: { rtt: number; offset: number } | null = null;
    for (let i = 0; i < 3; i++) {
      const sent = Date.now();
      try {
        const pong = await this.request<TimePong>({ t: "time:ping", d: { clientSentAt: sent } });
        const now = Date.now();
        const rtt = now - sent;
        const offset = pong.serverNow - (sent + rtt / 2);
        if (!best || rtt < best.rtt) best = { rtt, offset };
      } catch {
        return;
      }
    }
    if (best) this.offset = best.offset;
  }

  serverNow(): number {
    return Date.now() + this.offset;
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }
}
