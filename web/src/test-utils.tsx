import type { ReactNode } from "react";
import { render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import "./i18n";
import App from "./App";

type Handler = (url: string, init?: RequestInit) => { status: number; body?: unknown } | undefined;

/** Replaces fetch with `handler`; unknown URLs answer 404. */
export function mockFetch(handler: Handler) {
  const calls: { url: string; init?: RequestInit }[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input.toString();
    calls.push({ url, init });
    const r = handler(url, init) ?? { status: 404, body: { error: { code: "NOT_FOUND", message: "" } } };
    return new Response(r.status === 204 ? null : JSON.stringify(r.body ?? null), {
      status: r.status,
      headers: { "Content-Type": "application/json" },
    });
  }) as typeof fetch;
  return calls;
}

export function renderApp(path: string, wrap?: (node: ReactNode) => ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = (
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>
  );
  return render(wrap ? wrap(tree) : tree);
}

export const ME = {
  id: "u1",
  email: "lan@uit.edu.vn",
  displayName: "Lan",
  role: "user",
  locale: "vi",
  csrfToken: "csrf-1",
};
