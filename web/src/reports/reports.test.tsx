import { fireEvent, screen } from "@testing-library/react";
import i18n from "../i18n";
import { ME, mockFetch, renderApp } from "../test-utils";

const SUMMARY = {
  id: "s1",
  name: "SE101.A" as string | null,
  quizTitle: "Địa lý",
  startedAt: "2026-11-23T09:00:00Z",
  endedAt: "2026-11-23T09:10:00Z",
  ended: true,
  playerCount: 2,
  averageScore: 950,
};
const REPORT = {
  session: SUMMARY,
  participation: 1,
  accuracy: 0.5,
  averageMs: 4000,
  questions: [
    { index: 0, kind: "single", text: "Thủ đô?", answered: 2, correct: 1, accuracy: 0.5, averageMs: 3000, hard: false },
    { index: 1, kind: "fill_blank", text: "Sông ___", answered: 1, correct: 0, accuracy: 0.2, averageMs: 6000, hard: true },
  ],
  players: [
    {
      id: "p1",
      nickname: "An",
      rank: 1,
      score: 1900,
      correctCount: 2,
      kicked: false,
      answers: [
        { questionIndex: 0, answer: "Hà Nội", correct: true, points: 950, responseMs: 2000 },
        { questionIndex: 1, answer: null, correct: false, points: 0, responseMs: null },
      ],
    },
  ],
};

beforeEach(async () => {
  await i18n.changeLanguage("vi");
});

test("the reports list links each game by its name and searches", async () => {
  const calls = mockFetch((url) =>
    url.endsWith("/me")
      ? { status: 200, body: ME }
      : url.includes("/sessions?page=1")
        ? { status: 200, body: { items: [SUMMARY, { ...SUMMARY, id: "s2", name: null }], page: 1, pageSize: 20, total: 2 } }
        : undefined,
  );
  renderApp("/reports");
  const link = await screen.findByText("SE101.A");
  expect(link.closest("a")!.getAttribute("href")).toBe("/reports/s1");
  expect(screen.getByText("(chưa đặt tên)").closest("a")!.getAttribute("href")).toBe("/reports/s2");
  expect(screen.getAllByText("Địa lý").length).toBe(2);

  fireEvent.change(screen.getByLabelText("Tìm theo tên lượt chơi hoặc bộ câu hỏi"), { target: { value: "SE101 A" } });
  await vi.waitFor(() => expect(calls.some((c) => c.url.endsWith("/sessions?page=1&q=SE101%20A"))).toBe(true));
});

test("a game can be renamed from its report", async () => {
  const calls = mockFetch((url, init) =>
    url.endsWith("/me")
      ? { status: 200, body: ME }
      : url.endsWith("/sessions/s1") && init?.method === "PATCH"
        ? { status: 200, body: { ...SUMMARY, name: "SE101.B" } }
        : url.endsWith("/sessions/s1")
          ? { status: 200, body: REPORT }
          : undefined,
  );
  renderApp("/reports/s1");
  expect(await screen.findByRole("heading", { name: "SE101.A" })).toBeTruthy();
  fireEvent.click(screen.getByText("Đổi tên"));
  fireEvent.change(screen.getByLabelText("Tên lượt chơi"), { target: { value: " SE101.B " } });
  fireEvent.click(screen.getByText("Lưu"));
  await vi.waitFor(() => expect(calls.some((c) => c.init?.method === "PATCH")).toBe(true));
  const patch = calls.find((c) => c.init?.method === "PATCH")!;
  expect(JSON.parse(String(patch.init!.body))).toEqual({ name: "SE101.B" });
});

test("a report shows hard questions, player answers, and downloads Excel", async () => {
  const assign = vi.fn();
  vi.stubGlobal("location", { ...window.location, assign });
  const calls = mockFetch((url, init) =>
    url.endsWith("/me")
      ? { status: 200, body: ME }
      : url.endsWith("/sessions/s1")
        ? { status: 200, body: REPORT }
        : url.endsWith("/sessions/s1/exports") && init?.method === "POST"
          ? { status: 200, body: { url: "/api/v1/exports/tok", expiresAt: "2026-11-23T09:20:00Z" } }
          : undefined,
  );
  renderApp("/reports/s1");
  expect(await screen.findByText("Khó")).toBeTruthy();
  expect(screen.getAllByText("50%").length).toBe(2);
  fireEvent.click(screen.getByText("An"));
  expect(await screen.findByText(/Không trả lời/)).toBeTruthy();

  fireEvent.click(screen.getByText("Tải Excel"));
  await vi.waitFor(() => expect(assign).toHaveBeenCalledWith("/api/v1/exports/tok"));
  const post = calls.find((c) => c.url.endsWith("/exports"))!;
  expect(JSON.parse(String(post.init!.body))).toEqual({ format: "xlsx" });
  vi.unstubAllGlobals();
});

test("someone else's report is not found", async () => {
  mockFetch((url) =>
    url.endsWith("/me")
      ? { status: 200, body: ME }
      : url.endsWith("/sessions/zz")
        ? { status: 404, body: { error: { code: "SESSION_NOT_FOUND", message: "" } } }
        : undefined,
  );
  renderApp("/reports/zz");
  expect(await screen.findByText("Không tìm thấy báo cáo.")).toBeTruthy();
});
