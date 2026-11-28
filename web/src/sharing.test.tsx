import { fireEvent, screen, waitFor } from "@testing-library/react";
import i18n from "./i18n";
import { splitEmails } from "./components/ShareDialog";
import { ME, mockFetch, renderApp } from "./test-utils";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

const later = new Date(Date.now() + 20 * 86_400_000).toISOString();
const mine = {
  items: [
    { id: "q1", title: "Arrays", status: "published", visibility: "public", questionCount: 3, sharedWith: 2, updatedAt: later, expiresAt: later },
  ],
  page: 1,
  pageSize: 20,
  total: 1,
};

test("pasted addresses are split and lower-cased", () => {
  expect(splitEmails(" A@x.vn, b@x.vn;c@x.vn\nD@X.VN ")).toEqual(["a@x.vn", "b@x.vn", "c@x.vn", "d@x.vn"]);
});

test("the owner shares a quiz with people and makes it private", async () => {
  const calls = mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/quizzes?")) return { status: 200, body: mine };
    if (url === "/api/v1/quizzes/q1/sharing" && init?.method === "PUT")
      return { status: 200, body: JSON.parse(String(init.body)) };
    if (url === "/api/v1/quizzes/q1/sharing")
      return { status: 200, body: { visibility: "public", emails: ["minh@uit.edu.vn", "an@uit.edu.vn"] } };
  });
  renderApp("/quizzes");
  expect(await screen.findByText("Public")).toBeTruthy();
  expect(screen.getByText("Shared with 2 people")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Share" }));
  expect(await screen.findByText("minh@uit.edu.vn")).toBeTruthy();
  const box = screen.getByRole("checkbox") as HTMLInputElement;
  expect(box.checked).toBe(true);
  fireEvent.click(box);
  fireEvent.click(screen.getByRole("button", { name: "Remove an@uit.edu.vn" }));
  fireEvent.change(screen.getByLabelText("Email address"), { target: { value: "Binh@uit.edu.vn, minh@uit.edu.vn" } });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  expect(screen.getByText("binh@uit.edu.vn")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const put = calls.find((c) => c.url === "/api/v1/quizzes/q1/sharing" && c.init?.method === "PUT")!;
  expect(JSON.parse(String(put.init!.body))).toEqual({
    visibility: "private",
    emails: ["minh@uit.edu.vn", "binh@uit.edu.vn"],
  });
});

test("a quiz shared with me can be copied or hosted", async () => {
  const calls = mockFetch((url, init) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/library?scope=shared"))
      return {
        status: 200,
        body: {
          items: [
            { id: "s1", title: "Pointers", description: "Chapter 8", ownerName: "Minh", coverMediaUrl: null, questionCount: 10, updatedAt: later },
          ],
          page: 1,
          pageSize: 20,
          total: 1,
        },
      };
    if (url === "/api/v1/quizzes/s1/duplicate" && init?.method === "POST")
      return { status: 201, body: { id: "c1", title: "Pointers (copy)" } };
    if (url === "/api/v1/games" && init?.method === "POST")
      return { status: 201, body: { sessionId: "g1", pin: "123456", joinUrl: "" } };
  });
  renderApp("/quizzes");
  fireEvent.click(await screen.findByRole("tab", { name: "Shared with me" }));
  expect(await screen.findByText("Pointers")).toBeTruthy();
  expect(screen.getByText(/by Minh/)).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Copy to my quizzes" }));
  expect(await screen.findByText(/"Pointers \(copy\)" is now in My quizzes/)).toBeTruthy();
  expect(screen.getByRole("link", { name: "Open it" }).getAttribute("href")).toBe("/quizzes/c1/edit");

  fireEvent.click(screen.getByRole("button", { name: "Host live" }));
  await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/games")).toBe(true));
  const game = calls.find((c) => c.url === "/api/v1/games")!;
  expect(JSON.parse(String(game.init!.body))).toEqual({ quizId: "s1" });
});

test("the public tab lists public quizzes", async () => {
  mockFetch((url) => {
    if (url === "/api/v1/me") return { status: 200, body: ME };
    if (url.startsWith("/api/v1/library?scope=public"))
      return { status: 200, body: { items: [], page: 1, pageSize: 20, total: 0 } };
  });
  renderApp("/quizzes?tab=public");
  expect(await screen.findByText("There are no public quizzes yet.")).toBeTruthy();
});
