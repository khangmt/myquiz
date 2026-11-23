import { fireEvent, screen, waitFor } from "@testing-library/react";
import i18n from "./i18n";
import { daysLeft } from "./pages/QuizList";
import { ME, mockFetch, renderApp } from "./test-utils";

const unauthenticated = { status: 401, body: { error: { code: "UNAUTHENTICATED", message: "" } } };

beforeEach(async () => {
  await i18n.changeLanguage("vi");
});

test("shows Vietnamese by default and switches to English", async () => {
  mockFetch(() => unauthenticated);
  renderApp("/");
  expect(screen.getByText("Tham gia trò chơi")).toBeTruthy();
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "en" } });
  expect(await screen.findByText("Join a game")).toBeTruthy();
});

test("PIN field keeps digits only", () => {
  mockFetch(() => unauthenticated);
  renderApp("/");
  const input = screen.getAllByLabelText(/PIN/)[0] as HTMLInputElement;
  fireEvent.change(input, { target: { value: "12a3b4" } });
  expect(input.value).toBe("1234");
});

test("my quizzes sends signed-out users to sign in", async () => {
  mockFetch(() => unauthenticated);
  renderApp("/quizzes");
  expect(await screen.findByText("Đăng nhập bằng Google")).toBeTruthy();
  const link = screen.getByText("Đăng nhập bằng Google").closest("a")!;
  expect(link.getAttribute("href")).toBe("/api/v1/auth/google?next=%2Fquizzes");
});

test("a refused sign-in explains why", async () => {
  mockFetch(() => unauthenticated);
  renderApp("/login?error=DOMAIN_NOT_ALLOWED");
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Tài khoản email này không được phép đăng nhập.",
  );
});

test("lists quizzes with their deletion date and a warning when close", async () => {
  const soon = new Date(Date.now() + 3 * 86_400_000).toISOString();
  const later = new Date(Date.now() + 20 * 86_400_000).toISOString();
  mockFetch((url) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/quizzes?"))
      return {
        status: 200,
        body: {
          items: [
            { id: "q1", title: "Địa lý", status: "draft", visibility: "private", questionCount: 3, updatedAt: soon, expiresAt: soon },
            { id: "q2", title: "Toán", status: "published", visibility: "private", questionCount: 1, updatedAt: later, expiresAt: later },
          ],
          page: 1,
          pageSize: 20,
          total: 2,
        },
      };
  });
  renderApp("/quizzes");
  expect(await screen.findByText("Địa lý")).toBeTruthy();
  expect(screen.getByText("còn 3 ngày")).toBeTruthy();
  expect(screen.queryByText("còn 20 ngày")).toBeNull();
  expect(screen.getByText(/3 câu hỏi/)).toBeTruthy();
  expect(screen.getByText(/xoá khỏi máy chủ 3 tuần/)).toBeTruthy();
});

test("a refused zip shows every problem in the user's language", async () => {
  const calls = mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/quizzes?")) return { status: 200, body: { items: [], page: 1, pageSize: 20, total: 0 } };
    if (url === "/api/v1/quizzes/import" && init?.method === "POST")
      return {
        status: 422,
        body: {
          error: { code: "IMPORT_INVALID", message: "" },
          errors: [
            { file: "quiz.json", question: 1, field: "timeLimitS", problem: "TIME_LIMIT" },
            { file: "media/q2.png", question: null, field: null, problem: "MISSING_IMAGE" },
          ],
        },
      };
  });
  renderApp("/quizzes");
  await screen.findByText("Bạn chưa có bộ câu hỏi nào.");
  const input = screen.getByTestId("zip-input") as HTMLInputElement;
  fireEvent.change(input, { target: { files: [new File(["x"], "quiz.zip")] } });
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("quiz.json · câu 2: Thời gian phải từ 5 đến 240 giây.");
  expect(alert.textContent).toContain("media/q2.png: Thiếu ảnh được nhắc tới trong quiz.json.");
  const upload = calls.find((c) => c.url === "/api/v1/quizzes/import")!;
  expect((upload.init!.headers as Record<string, string>)["X-CSRF-Token"]).toBe("csrf-1");
});

test("days left never goes below zero", () => {
  const now = Date.parse("2026-12-01T00:00:00Z");
  expect(daysLeft("2026-12-08T00:00:00Z", now)).toBe(7);
  expect(daysLeft("2026-11-01T00:00:00Z", now)).toBe(0);
});

test("signing out calls the API and returns home", async () => {
  const calls = mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/auth/logout" && init?.method === "POST") return { status: 204 };
    if (url.startsWith("/api/v1/quizzes?")) return { status: 200, body: { items: [], page: 1, pageSize: 20, total: 0 } };
  });
  renderApp("/quizzes");
  fireEvent.click(await screen.findByText("Đăng xuất"));
  await waitFor(() => expect(screen.getByText("Tham gia trò chơi")).toBeTruthy());
  expect(calls.some((c) => c.url === "/api/v1/auth/logout")).toBe(true);
});
