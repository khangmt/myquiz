// A small fetch wrapper for the REST API (design section 6). Every error
// comes back as { error: { code, message } }; callers switch on the code and
// the UI translates it (errors.<CODE> in the i18n files).

import type { Me } from "../generated/Me";

export class ApiFailure extends Error {
  constructor(
    public status: number,
    public code: string,
    /** The whole JSON body, e.g. with `problems` or `errors` lists. */
    public body: unknown,
  ) {
    super(code);
  }
}

let csrfToken: string | null = null;

/** Set from GET /me; sent on every request that changes something. */
export function setCsrfToken(token: string | null) {
  csrfToken = token;
}

type Options = {
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  json?: unknown;
  form?: FormData;
  headers?: Record<string, string>;
  signal?: AbortSignal;
};

export async function api<T>(path: string, opts: Options = {}): Promise<T> {
  const method = opts.method ?? "GET";
  const headers: Record<string, string> = { Accept: "application/json", ...opts.headers };
  let body: BodyInit | undefined;
  if (opts.json !== undefined) {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(opts.json);
  } else if (opts.form) {
    body = opts.form;
  }
  if (method !== "GET" && csrfToken) headers["X-CSRF-Token"] = csrfToken;

  const res = await fetch(`/api/v1${path}`, {
    method,
    headers,
    body,
    credentials: "same-origin",
    signal: opts.signal,
  });
  if (res.status === 204) return undefined as T;
  const data: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    const code =
      (data as { error?: { code?: string } } | null)?.error?.code ??
      (res.status === 413 ? "FILE_TOO_LARGE" : "INTERNAL");
    throw new ApiFailure(res.status, code, data);
  }
  return data as T;
}

/** The signed-in user, or null when nobody is signed in. */
export async function fetchMe(): Promise<Me | null> {
  try {
    const me = await api<Me>("/me");
    setCsrfToken(me.csrfToken);
    return me;
  } catch (e) {
    if (e instanceof ApiFailure && e.status === 401) {
      setCsrfToken(null);
      return null;
    }
    throw e;
  }
}

/** Where the Google sign-in starts; `next` is where to land afterwards. */
export function googleSignInUrl(next: string): string {
  return `/api/v1/auth/google?next=${encodeURIComponent(next)}`;
}
