import { fireEvent, screen, waitFor } from "@testing-library/react";
import i18n from "./i18n";
import { daysLeft } from "./pages/QuizList";
import { ME, mockFetch, renderApp } from "./test-utils";

const unauthenticated = { status: 401, body: { error: { code: "UNAUTHENTICATED", message: "" } } };

beforeEach(async () => {
  await i18n.changeLanguage("vi");
});

test("starts in English and switches to Vietnamese", async () => {
  expect(i18n.options.lng).toBe("en");
  await i18n.changeLanguage("en");
  mockFetch(() => unauthenticated);
  renderApp("/");
  expect(screen.getByText("Enter")).toBeTruthy();
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "vi" } });
  expect(await screen.findByText("Tham gia trò chơi")).toBeTruthy();
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
  expect(alert.textContent).toContain("media/q2.png: Thiếu ảnh được nhắc tới trong tệp câu hỏi.");
  const upload = calls.find((c) => c.url === "/api/v1/quizzes/import")!;
  expect((upload.init!.headers as Record<string, string>)["X-CSRF-Token"]).toBe("csrf-1");
});

test("a quiz downloads as YAML and YAML files can be imported", async () => {
  const calls = mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/quizzes?"))
      return {
        status: 200,
        body: {
          items: [
            { id: "q1", title: "Mảng", status: "draft", visibility: "private", questionCount: 3, updatedAt: "2026-11-28T00:00:00Z", expiresAt: "2026-12-19T00:00:00Z" },
          ],
          page: 1,
          pageSize: 20,
          total: 1,
        },
      };
    if (url.startsWith("/api/v1/quizzes/q1/export") && init?.method === "POST")
      return { status: 422, body: { error: { code: "QUIZ_INVALID", message: "" }, problems: [] } };
    if (url === "/api/v1/quizzes/import" && init?.method === "POST")
      return {
        status: 422,
        body: {
          error: { code: "IMPORT_INVALID", message: "" },
          errors: [{ file: "media/q1.png", question: null, field: null, problem: "IMAGE_NEEDS_ZIP" }],
        },
      };
  });
  renderApp("/quizzes");
  fireEvent.click(await screen.findByText("Tải YAML"));
  await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/quizzes/q1/export?format=yaml")).toBe(true));
  fireEvent.click(screen.getByText("Tải zip"));
  await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/quizzes/q1/export")).toBe(true));

  const input = screen.getByTestId("zip-input") as HTMLInputElement;
  expect(input.accept).toContain(".yaml");
  fireEvent.change(input, { target: { files: [new File(["quiz: x"], "quiz.yaml")] } });
  expect(await screen.findByText(/hãy nén quiz.yaml cùng thư mục media/)).toBeTruthy();
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

test("signed-in users are warned about quizzes deleted within 3 days", async () => {
  const soon = new Date(Date.now() + 2 * 86_400_000 - 60_000).toISOString();
  mockFetch((url) =>
    url.endsWith("/me")
      ? { status: 200, body: ME }
      : url.endsWith("/quizzes/expiring")
        ? { status: 200, body: [{ id: "q1", title: "Toán lớp 5", expiresAt: soon }] }
        : url.includes("/quizzes?")
          ? { status: 200, body: { items: [], page: 1, pageSize: 20, total: 0 } }
          : undefined,
  );
  localStorage.clear();
  renderApp("/quizzes");
  const banner = await screen.findByText(/sẽ bị xoá trong 2 ngày nữa/);
  expect(banner.closest("[role=status]")!.textContent).toContain("Toán lớp 5");
  fireEvent.click(screen.getByText("Ẩn hôm nay"));
  await waitFor(() => expect(screen.queryByText(/sẽ bị xoá trong/)).toBeNull());
});

test("a quiz in the list can be hosted live without opening the editor", async () => {
  const later = new Date(Date.now() + 20 * 86_400_000).toISOString();
  const calls = mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/quizzes?"))
      return {
        status: 200,
        body: {
          items: [{ id: "q1", title: "Địa lý", status: "draft", visibility: "private", questionCount: 3, updatedAt: later, expiresAt: later }],
          page: 1,
          pageSize: 20,
          total: 1,
        },
      };
    if (url === "/api/v1/quizzes/q1/publish" && init?.method === "POST") return { status: 200, body: { id: "q1", status: "published" } };
    if (url === "/api/v1/games" && init?.method === "POST") return { status: 201, body: { pin: "654321", sessionId: "s1" } };
  });
  renderApp("/quizzes");
  fireEvent.click(await screen.findByRole("button", { name: "Tổ chức trò chơi" }));
  // A draft is published first, then the lobby opens.
  await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/games")).toBe(true));
  const order = calls.map((c) => c.url).filter((u) => u.endsWith("/publish") || u.endsWith("/games"));
  expect(order).toEqual(["/api/v1/quizzes/q1/publish", "/api/v1/games"]);
  expect(JSON.parse(String(calls.find((c) => c.url === "/api/v1/games")!.init!.body))).toEqual({ quizId: "q1" });
});
