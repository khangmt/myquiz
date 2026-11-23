import { fireEvent, screen, waitFor } from "@testing-library/react";
import i18n from "../i18n";
import { ME, mockFetch, renderApp } from "../test-utils";

const ADMIN = { ...ME, role: "admin" };
const USER = {
  id: "u2",
  email: "binh@uit.edu.vn",
  displayName: "Bình",
  role: "user",
  status: "active",
  createdAt: "2026-11-01T02:00:00Z",
  quizCount: 3,
  gameCount: 1,
};

beforeEach(async () => {
  await i18n.changeLanguage("vi");
});

test("non-admins get no admin link and no admin page", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : undefined));
  renderApp("/admin");
  expect(await screen.findByText("Trang này chỉ dành cho quản trị viên.")).toBeTruthy();
  expect(screen.queryByRole("link", { name: "Quản trị" })).toBeNull();
});

test("an admin searches users and suspends one with CSRF", async () => {
  vi.stubGlobal("confirm", () => true);
  let status = "active";
  const calls = mockFetch((url, init) => {
    if (url.endsWith("/me")) return { status: 200, body: ADMIN };
    if (url.includes("/admin/users?"))
      return { status: 200, body: { items: [{ ...USER, status }], page: 1, pageSize: 50, total: 1 } };
    if (url.endsWith("/admin/users/u2/suspend") && init?.method === "POST") {
      status = "suspended";
      return { status: 204 };
    }
    return undefined;
  });
  renderApp("/admin");
  expect(await screen.findByRole("link", { name: "Quản trị" })).toBeTruthy();
  expect(await screen.findByText("binh@uit.edu.vn")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Tìm theo email hoặc tên"), { target: { value: "bình" } });
  await waitFor(() => expect(calls.some((c) => c.url.includes("q=b%C3%ACnh"))).toBe(true));
  fireEvent.click(await screen.findByRole("button", { name: "Khóa" }));
  expect(await screen.findByText("Đã khóa")).toBeTruthy();
  const post = calls.find((c) => c.url.endsWith("/suspend"))!;
  expect(new Headers(post.init!.headers).get("X-CSRF-Token")).toBe("csrf-1");
  expect(screen.getByRole("button", { name: "Mở khóa" })).toBeTruthy();
});

test("the audit log filters by action and date", async () => {
  const calls = mockFetch((url) => {
    if (url.endsWith("/me")) return { status: 200, body: ADMIN };
    if (url.includes("/admin/users?")) return { status: 200, body: { items: [], page: 1, pageSize: 50, total: 0 } };
    if (url.includes("/admin/audit?"))
      return {
        status: 200,
        body: {
          items: [
            {
              id: 7,
              at: "2026-11-23T03:00:00Z",
              actorEmail: "lan@uit.edu.vn",
              action: "user_suspended",
              targetType: "user",
              targetId: "u2",
              ipHash: null,
              details: { email: "binh@uit.edu.vn" },
            },
          ],
          page: 1,
          pageSize: 50,
          total: 1,
        },
      };
    return undefined;
  });
  renderApp("/admin");
  fireEvent.click(await screen.findByRole("tab", { name: "Nhật ký" }));
  expect(await screen.findByText(/binh@uit.edu.vn/)).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Hành động"), { target: { value: "user_suspended" } });
  fireEvent.change(screen.getByLabelText("Từ"), { target: { value: "2026-11-20" } });
  await waitFor(() =>
    expect(calls.some((c) => c.url.includes("action=user_suspended") && c.url.includes("from=2026-11-20"))).toBe(true),
  );
});
