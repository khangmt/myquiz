import { fireEvent, screen, waitFor } from "@testing-library/react";
import i18n from "../i18n";
import { ME, mockFetch, renderApp } from "../test-utils";

const DOC = {
  id: "q1",
  title: "Địa lý",
  description: "",
  coverMediaId: null,
  coverMediaUrl: null,
  visibility: "private",
  status: "draft",
  version: 4,
  updatedAt: "2026-11-23T00:00:00Z",
  expiresAt: "2026-12-14T00:00:00Z",
  questions: [
    {
      type: "single",
      text: "Thủ đô?",
      mediaId: null,
      mediaUrl: null,
      markdown: null,
      timeLimitS: 20,
      points: 1000,
      options: [
        { text: "Hà Nội", correct: true },
        { text: "Huế", correct: false },
      ],
      acceptedAnswers: [],
      ignoreDiacritics: false,
    },
  ],
};

beforeEach(async () => {
  await i18n.changeLanguage("vi");
});

test("autosaves with the version and shows the deletion date", async () => {
  let saved: { body: unknown; ifMatch: string } | null = null;
  mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1" && init?.method === "PUT") {
      saved = {
        body: JSON.parse(init.body as string),
        ifMatch: (init.headers as Record<string, string>)["If-Match"],
      };
      return { status: 200, body: { ...DOC, version: 5, title: "Địa lý 2" } };
    }
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  renderApp("/quizzes/q1/edit");
  const title = (await screen.findByLabelText("Tên bộ câu hỏi")) as HTMLInputElement;
  expect(screen.getByText(/sẽ bị xoá khỏi máy chủ vào 14 tháng 12, 2026/)).toBeTruthy();
  fireEvent.change(title, { target: { value: "Địa lý 2" } });
  expect(screen.getByRole("status").textContent).toContain("Chưa lưu");
  await waitFor(() => expect(saved).not.toBeNull(), { timeout: 3000 });
  expect(saved!.ifMatch).toBe('"4"');
  expect((saved!.body as { title: string }).title).toBe("Địa lý 2");
  await waitFor(() => expect(screen.getByText("Đã lưu")).toBeTruthy());
});

test("a save from another tab stops autosave and offers a reload", async () => {
  mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1" && init?.method === "PUT")
      return { status: 412, body: { error: { code: "VERSION_CONFLICT", message: "" } } };
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  renderApp("/quizzes/q1/edit");
  fireEvent.change(await screen.findByLabelText("Tên bộ câu hỏi"), { target: { value: "X" } });
  expect(await screen.findByText("Tải lại", {}, { timeout: 3000 })).toBeTruthy();
});

test("publish problems are shown on the question they belong to", async () => {
  mockFetch((url) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1/publish")
      return {
        status: 422,
        body: { error: { code: "QUIZ_INVALID", message: "" }, problems: [{ question: 0, code: "NO_CORRECT_OPTION" }] },
      };
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  renderApp("/quizzes/q1/edit");
  fireEvent.click(await screen.findByText("Xuất bản"));
  const alerts = await screen.findAllByText("Hãy đánh dấu ít nhất một đáp án đúng.");
  expect(alerts.length).toBeGreaterThan(0);
});

test("switching a question to true/false gives the two fixed options", async () => {
  mockFetch((url) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  renderApp("/quizzes/q1/edit");
  fireEvent.change(await screen.findByLabelText("Loại câu hỏi"), { target: { value: "true_false" } });
  expect((screen.getByLabelText("Lựa chọn 1") as HTMLInputElement).value).toBe("Đúng");
  expect((screen.getByLabelText("Lựa chọn 2") as HTMLInputElement).value).toBe("Sai");
  expect(screen.queryByText("+ Thêm lựa chọn")).toBeNull();
});

test("a question can show code instead of an image, saved as Markdown", async () => {
  let saved: { questions: { markdown: string | null; mediaId: string | null }[] } | null = null;
  mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1" && init?.method === "PUT") {
      saved = JSON.parse(init.body as string);
      return { status: 200, body: { ...DOC, version: 5 } };
    }
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  renderApp("/quizzes/q1/edit");
  fireEvent.click(await screen.findByRole("tab", { name: "Mã nguồn / Markdown" }));
  const box = screen.getByLabelText("Mã nguồn / Markdown") as HTMLTextAreaElement;
  expect(box.value).toBe("```c\n\n```");
  fireEvent.change(box, { target: { value: "```c\nint x = 1;\n```\n<script>alert(1)</script>" } });
  const preview = screen.getByTestId("markdown-preview");
  expect(preview.querySelector("code.language-c")?.textContent).toBe("int x = 1;");
  expect(preview.querySelector("script")).toBeNull();
  await waitFor(() => expect(saved).not.toBeNull(), { timeout: 3000 });
  expect(saved!.questions[0].markdown).toContain("int x = 1;");
  expect(saved!.questions[0].mediaId).toBeNull();
});

test("a new question is scrolled into view with the cursor in its text", async () => {
  mockFetch((url) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  const scrolled: Element[] = [];
  Element.prototype.scrollIntoView = function (this: Element) {
    scrolled.push(this);
  };
  renderApp("/quizzes/q1/edit");
  fireEvent.click(await screen.findByRole("button", { name: "+ Đúng/Sai" }));
  const card = await screen.findByRole("article", { name: "Câu 2" });
  await waitFor(() => expect(scrolled).toContain(card));
  expect(document.activeElement).toBe(card.querySelector("textarea"));
});

test("with the add row off screen, a corner button opens the choices and closes after adding", async () => {
  mockFetch((url) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url === "/api/v1/quizzes/q1") return { status: 200, body: DOC };
  });
  // The add row after the last question is scrolled out of view.
  globalThis.IntersectionObserver = class {
    constructor(private cb: IntersectionObserverCallback) {}
    observe() {
      this.cb([{ isIntersecting: false } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
    disconnect() {}
  } as unknown as typeof IntersectionObserver;
  renderApp("/quizzes/q1/edit");
  const toggle = await screen.findByRole("button", { name: "+ Thêm câu hỏi" });
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.click(toggle);
  fireEvent.click(screen.getByRole("menuitem", { name: "+ Điền từ" }));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(await screen.findByRole("article", { name: "Câu 2" })).toBeTruthy();
  // @ts-expect-error jsdom has none; later tests expect that.
  delete globalThis.IntersectionObserver;
});
