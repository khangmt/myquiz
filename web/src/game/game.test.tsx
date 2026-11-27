import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import i18n from "../i18n";
import { ME, mockFetch, renderApp } from "../test-utils";

/** A stand-in WebSocket: records frames and lets a test answer them. */
class FakeSocket {
  static OPEN = 1;
  static all: FakeSocket[] = [];
  readyState = 0;
  sent: { t: string; id?: number; d: unknown }[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(public url: string) {
    FakeSocket.all.push(this);
    queueMicrotask(() => {
      this.readyState = 1;
      this.onopen?.();
    });
  }
  send(raw: string) {
    this.sent.push(JSON.parse(raw));
  }
  close() {
    this.readyState = 3;
    this.onclose?.();
  }
  /** Acks the latest request named `t`. */
  ack(t: string, d: unknown, ok = true, code: string | null = null) {
    const req = [...this.sent].reverse().find((m) => m.t === t);
    if (!req) throw new Error(`no ${t} sent`);
    act(() => this.onmessage?.({ data: JSON.stringify({ t: "ack", id: req.id, ok, d, code }) }));
  }
  push(t: string, d: unknown) {
    act(() => this.onmessage?.({ data: JSON.stringify({ t, d }) }));
  }
}

const lobby = { pin: "123456", state: { state: "lobby" }, locked: false, players: [], question: null, me: null, reportId: null, reveal: null, result: null, top: null };
const question = (over: object = {}) => ({
  startsAt: Date.now() - 1000,
  endsAt: Date.now() + 20_000,
  index: 0,
  total: 2,
  type: "single",
  text: "Thủ đô của Việt Nam?",
  media: null,
  options: [
    { id: 0, text: "Hà Nội" },
    { id: 1, text: "Huế" },
  ],
  timeLimit: 20,
  points: 1000,
  ...over,
});

async function socket(): Promise<FakeSocket> {
  await waitFor(() => expect(FakeSocket.all.at(-1)?.sent.length).toBeGreaterThan(0));
  return FakeSocket.all.at(-1)!;
}

beforeEach(async () => {
  FakeSocket.all = [];
  vi.stubGlobal("WebSocket", FakeSocket);
  localStorage.clear();
  await i18n.changeLanguage("vi");
});
afterEach(() => vi.unstubAllGlobals());

test("a player joins with a nickname, answers, and sees the result", async () => {
  mockFetch((url) => (url.endsWith("/games/pin/123456") ? { status: 200, body: { pin: "123456", title: "Địa lý", locked: false } } : undefined));
  renderApp("/join?pin=123456");
  fireEvent.change(await screen.findByLabelText("Biệt danh"), { target: { value: "Lan" } });
  fireEvent.click(screen.getByText("Vào chơi"));

  const ws = await socket();
  const join = ws.sent.find((m) => m.t === "player:join")!;
  expect(join.d).toMatchObject({ pin: "123456", nickname: "Lan" });
  ws.ack("player:join", { playerId: "p1", token: "tok-1", snapshot: { ...lobby, me: { playerId: "p1", nickname: "Lan", score: 0, rank: 1, answered: false } } });
  expect(await screen.findByText("Bạn đã vào phòng!")).toBeTruthy();
  expect(localStorage.getItem("myquiz.player.123456")).toBe("tok-1");

  ws.push("question:start", question());
  fireEvent.click(await screen.findByText("Hà Nội"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "answer:submit", d: { index: 0, optionIds: [0], text: null } });
  expect(await screen.findByText("Đã gửi câu trả lời!")).toBeTruthy();
  ws.ack("answer:submit", { accepted: true });

  ws.push("question:result", { index: 0, answered: true, correct: true, points: 950, total: 950, rank: 1, correctOptionIds: [0] });
  expect(await screen.findByText("Chính xác!")).toBeTruthy();
  expect(screen.getByText("+950")).toBeTruthy();
});

test("a reload resumes with the saved token", async () => {
  localStorage.setItem("myquiz.player.123456", "tok-9");
  mockFetch(() => undefined);
  renderApp("/join?pin=123456");
  const ws = await socket();
  expect(ws.sent[0]).toMatchObject({ t: "player:resume", d: { pin: "123456", token: "tok-9" } });
  ws.ack("player:resume", { ...lobby, state: { state: "question", index: 0 }, question: question(), me: { playerId: "p1", nickname: "Lan", score: 0, rank: 1, answered: true } });
  expect(await screen.findByText("Đã gửi câu trả lời!")).toBeTruthy();
});

test("a taken nickname sends the player back to choose another", async () => {
  mockFetch((url) => (url.endsWith("/games/pin/123456") ? { status: 200, body: { pin: "123456", title: "Địa lý", locked: false } } : undefined));
  renderApp("/join?pin=123456");
  fireEvent.change(await screen.findByLabelText("Biệt danh"), { target: { value: "Lan" } });
  fireEvent.click(screen.getByText("Vào chơi"));
  const ws = await socket();
  ws.ack("player:join", null, false, "NICK_TAKEN");
  expect(await screen.findByText("Biệt danh này đã có người dùng.")).toBeTruthy();
  expect(await screen.findByLabelText("Biệt danh")).toBeTruthy();
});

test("a returning player on a new phone waits for the host, then continues", async () => {
  mockFetch((url) => (url.endsWith("/games/pin/123456") ? { status: 200, body: { pin: "123456", title: "Địa lý", locked: true } } : undefined));
  renderApp("/join?pin=123456");
  // Locked: the name form still shows, for players already in the game.
  expect(await screen.findByText(/Phòng chờ đã khóa/)).toBeTruthy();
  fireEvent.change(await screen.findByLabelText("Biệt danh"), { target: { value: "Lan" } });
  fireEvent.click(screen.getByText("Vào chơi"));
  const ws = await socket();
  ws.ack("player:join", null, false, "REJOIN_WAIT");
  expect(await screen.findByText("Tên này đã có trong trò chơi")).toBeTruthy();
  // It asks again by itself until the host says yes.
  await waitFor(() => expect(ws.sent.filter((m) => m.t === "player:join").length).toBe(2), { timeout: 4000 });
  ws.ack("player:join", { playerId: "p1", token: "tok-2", snapshot: { ...lobby, me: { playerId: "p1", nickname: "Lan", score: 900, rank: 1, answered: false } } });
  expect(await screen.findByText("Bạn đã vào phòng!")).toBeTruthy();
  expect(localStorage.getItem("myquiz.player.123456")).toBe("tok-2");
});

test("the host lets a returning player back in", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : url.endsWith("/config") ? { status: 200, body: { joinUrl: null } } : undefined));
  renderApp("/host/123456");
  const ws = await socket();
  ws.ack("host:attach", { ...lobby, players: [{ id: "p1", nickname: "Lan", avatar: "bunny" }] });
  ws.push("rejoin:request", { playerId: "p1", nickname: "Lan", avatar: "bunny" });
  ws.push("rejoin:request", { playerId: "p1", nickname: "Lan", avatar: "bunny" });
  expect(await screen.findAllByText("Lan muốn vào lại từ thiết bị mới.")).toHaveLength(1);
  fireEvent.click(screen.getByText("Cho vào"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:rejoin", d: { playerId: "p1" } });
  expect(screen.queryByText("Lan muốn vào lại từ thiết bị mới.")).toBeNull();
});

test("a wrong PIN is explained", async () => {
  mockFetch(() => undefined);
  renderApp("/join?pin=999999");
  expect(await screen.findByText("Mã PIN không đúng hoặc trò chơi đã kết thúc.")).toBeTruthy();
});

test("the host runs the lobby, starts, and moves on after the reveal", async () => {
  mockFetch((url) =>
    url.endsWith("/me") ? { status: 200, body: ME } : url.endsWith("/config") ? { status: 200, body: { joinUrl: "http://192.168.1.20:8080" } } : undefined,
  );
  renderApp("/host/123456");
  const ws = await socket();
  expect(ws.sent[0]).toMatchObject({ t: "host:attach", d: { pin: "123456" } });
  ws.ack("host:attach", { ...lobby, reportId: "s-42", players: [{ id: "p1", nickname: "Lan" }] });
  expect(await screen.findByText("123456")).toBeTruthy();
  expect(screen.getByText("1 người chơi")).toBeTruthy();
  expect(await screen.findByText("http://192.168.1.20:8080/join?pin=123456")).toBeTruthy();
  expect((await screen.findByAltText("Mã QR để vào trò chơi")).getAttribute("src")).toMatch(/^data:image\/png;base64,/);

  fireEvent.click(screen.getByLabelText("Tuỳ chọn cho Lan"));
  fireEvent.click(screen.getByLabelText("Mời Lan ra"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:kick", d: { playerId: "p1" } });
  ws.ack("host:kick", {});

  await waitFor(() => expect((screen.getByText("Bắt đầu") as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByText("Bắt đầu"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:start" });
  ws.ack("host:start", {});
  ws.push("question:start", question());
  ws.push("question:progress", { index: 0, answered: 1, connected: 3 });
  expect(await screen.findByText("1/3 đã trả lời")).toBeTruthy();

  ws.push("question:reveal", { index: 0, correctOptionIds: [0], acceptedAnswers: [], distribution: [{ optionId: 0, count: 1 }], topTextAnswers: [] });
  fireEvent.click(await screen.findByText("Tiếp"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:next", d: { expect: { state: "reveal", index: 0 } } });
  ws.ack("host:next", {});
  ws.push("leaderboard:show", { top: [{ nickname: "Lan", score: 950 }] });
  expect(await screen.findByText("Bảng xếp hạng")).toBeTruthy();
  expect(screen.getByText("950 điểm")).toBeTruthy();

  ws.push("game:ended", { reason: "finished" });
  expect((await screen.findByText("Xem báo cáo")).getAttribute("href")).toBe("/reports/s-42");
});

test("a player who runs out of time sees time's up, not wrong", async () => {
  localStorage.setItem("myquiz.player.123456", "tok-9");
  mockFetch(() => undefined);
  renderApp("/join?pin=123456");
  const ws = await socket();
  ws.ack("player:resume", { ...lobby, me: { playerId: "p1", nickname: "Lan", score: 0, rank: 1, answered: false } });
  ws.push("question:result", { index: 0, answered: false, correct: false, points: 0, total: 0, rank: 2, correctOptionIds: [0] });
  expect(await screen.findByText("Hết giờ!")).toBeTruthy();
  expect(screen.queryByText("Chưa đúng")).toBeNull();
});

test("a host who reloads during the reveal, leaderboard or podium sees that screen again", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : url.endsWith("/config") ? { status: 200, body: { joinUrl: null } } : undefined));
  renderApp("/host/123456");
  let ws = await socket();
  ws.ack("host:attach", {
    ...lobby,
    state: { state: "reveal", index: 0 },
    question: question(),
    reveal: { index: 0, correctOptionIds: [0], acceptedAnswers: [], distribution: [{ optionId: 0, count: 2 }], topTextAnswers: [] },
  });
  expect(await screen.findByText("Thủ đô của Việt Nam?")).toBeTruthy();
  fireEvent.click(await screen.findByText("Tiếp"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:next", d: { expect: { state: "reveal", index: 0 } } });

  ws.close();
  ws = await waitFor(() => {
    const s = FakeSocket.all.at(-1)!;
    if (s === ws || !s.sent.length) throw new Error("not reconnected yet");
    return s;
  }, { timeout: 5000 });
  ws.ack("host:attach", { ...lobby, state: { state: "podium" }, top: [{ nickname: "An", score: 1972 }, { nickname: "Chi", score: 1734 }] });
  expect(await screen.findByText("Vinh danh")).toBeTruthy();
  expect(screen.getByText(/An/)).toBeTruthy();
  expect(screen.getByText(/1972/)).toBeTruthy();
});

test("a player who reloads on the leaderboard or during the reveal sees it again", async () => {
  localStorage.setItem("myquiz.player.123456", "tok-9");
  mockFetch(() => undefined);
  renderApp("/join?pin=123456");
  const ws = await socket();
  const me = { playerId: "p1", nickname: "Lan", score: 950, rank: 1, answered: false };
  ws.ack("player:resume", {
    ...lobby,
    state: { state: "reveal", index: 0 },
    me,
    result: { index: 0, answered: true, correct: true, points: 950, total: 950, rank: 1, correctOptionIds: [0] },
  });
  expect(await screen.findByText("Chính xác!")).toBeTruthy();
});

test("a player picks an avatar in the game's style and joins with it", async () => {
  mockFetch((url) => (url.endsWith("/games/pin/123456") ? { status: 200, body: { pin: "123456", title: "Địa lý", locked: false, theme: "anime" } } : undefined));
  const { container } = renderApp("/join?pin=123456");
  fireEvent.change(await screen.findByLabelText("Biệt danh"), { target: { value: "Lan" } });
  expect(container.querySelector('.game-stage[data-theme="anime"]')).toBeTruthy();
  fireEvent.click(screen.getByLabelText("froggy"));
  fireEvent.click(screen.getByText("Vào chơi"));

  const ws = await socket();
  expect(ws.sent.find((m) => m.t === "player:join")!.d).toMatchObject({ nickname: "Lan", avatar: "froggy" });
  expect(localStorage.getItem("myquiz.avatar")).toBe("froggy");
  ws.ack("player:join", { playerId: "p1", token: "t", snapshot: { ...lobby, theme: "anime", me: { playerId: "p1", nickname: "Lan", avatar: "froggy", score: 0, rank: 1, answered: false } } });
  expect(await screen.findByText("Bạn đã vào phòng!")).toBeTruthy();
  ws.push("game:theme", { theme: "cute" });
  await waitFor(() => expect(container.querySelector('.game-stage[data-theme="cute"]')).toBeTruthy());
});

test("the host picks the style in the lobby", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : undefined));
  const { container } = renderApp("/host/123456");
  const ws = await socket();
  ws.ack("host:attach", { ...lobby, theme: "cute", reportId: "s-42", players: [{ id: "p1", nickname: "Lan", avatar: "kitty" }] });
  fireEvent.click(await screen.findByRole("radio", { name: /Anime/ }));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:theme", d: { theme: "anime" } });
  ws.ack("host:theme", {});
  ws.push("game:theme", { theme: "anime" });
  await waitFor(() => expect(screen.getByRole("radio", { name: /Anime/ }).getAttribute("aria-checked")).toBe("true"));
  expect(container.querySelector('.game-stage[data-theme="anime"]')).toBeTruthy();
});

test("the avatar ids match the server's list", async () => {
  const { readFileSync } = await import("node:fs");
  const { AVATAR_IDS } = await import("./avatars");
  const rust = readFileSync("../crates/protocol/src/ws.rs", "utf8");
  const list = rust.match(/pub const AVATARS: &\[&str\] = &\[([^\]]*)\]/)![1];
  expect([...list.matchAll(/"([a-z]+)"/g)].map((m) => m[1])).toEqual(AVATAR_IDS);
});

test("the chat emoji ids match the server's list", async () => {
  const { readFileSync } = await import("node:fs");
  const { CHAT_EMOJI_IDS, CHAT_MAX_CHARS } = await import("./chat");
  const rust = readFileSync("../crates/protocol/src/ws.rs", "utf8");
  const list = rust.match(/pub const CHAT_EMOJIS: &\[&str\] = &\[([^\]]*)\]/)![1];
  expect([...list.matchAll(/"([a-z]+)"/g)].map((m) => m[1])).toEqual(CHAT_EMOJI_IDS);
  expect(rust).toContain(`pub const CHAT_MAX_CHARS: usize = ${CHAT_MAX_CHARS};`);
});

test("the host sees chat as popups and can switch chat to emoji only", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : undefined));
  renderApp("/host/123456");
  const ws = await socket();
  ws.ack("host:attach", { ...lobby, theme: "cute", chat: "open", reportId: "s-42", players: [{ id: "p1", nickname: "Lan", avatar: "owl" }] });
  await screen.findByText("123456");

  ws.push("chat:message", { playerId: "p1", nickname: "Lan", avatar: "owl", text: "Chào cô!", emoji: null });
  expect((await screen.findByTestId("chat-bubble")).textContent).toContain("Chào cô!");

  fireEvent.click(screen.getByRole("radio", { name: "Chỉ emoji" }));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:chat", d: { mode: "emoji" } });
  ws.ack("host:chat", {});
  ws.push("game:chat", { mode: "emoji" });
  await waitFor(() => expect(screen.getByRole("radio", { name: "Chỉ emoji" }).getAttribute("aria-checked")).toBe("true"));

  // Hiding clears this screen and keeps new popups away.
  fireEvent.click(screen.getByText("Ẩn tin nhắn"));
  expect(screen.queryByTestId("chat-bubble")).toBeNull();
  ws.push("chat:message", { playerId: "p1", nickname: "Lan", avatar: "owl", text: "Again", emoji: null });
  expect(screen.queryByTestId("chat-bubble")).toBeNull();
});

test("a player sends a message from the lobby, and only emoji after answering", async () => {
  mockFetch((url) => (url.endsWith("/games/pin/123456") ? { status: 200, body: { pin: "123456", title: "Địa lý", locked: false } } : undefined));
  renderApp("/join?pin=123456");
  fireEvent.change(await screen.findByLabelText("Biệt danh"), { target: { value: "Lan" } });
  fireEvent.click(screen.getByText("Vào chơi"));
  const ws = await socket();
  ws.ack("player:join", { playerId: "p1", token: "tok-1", snapshot: { ...lobby, chat: "open", me: { playerId: "p1", nickname: "Lan", avatar: "owl", score: 0, rank: 1, answered: false } } });

  fireEvent.change(await screen.findByLabelText("Nhắn gì đó lên màn hình…"), { target: { value: "  Xin chào  " } });
  fireEvent.click(screen.getByText("Gửi"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "player:chat", d: { text: "Xin chào" } });
  ws.ack("player:chat", {});
  expect(await screen.findByText("Đã gửi!")).toBeTruthy();

  fireEvent.click(screen.getByLabelText("Vỗ tay"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "player:chat", d: { emoji: "clap" } });
  ws.ack("player:chat", null, false, "CHAT_SLOW");
  expect(await screen.findByText("Chậm lại chút nhé")).toBeTruthy();

  // While answering: no chat at all.
  ws.push("question:start", question());
  await screen.findByText("Hà Nội");
  expect(screen.queryByTestId("chat-bar")).toBeNull();
  fireEvent.click(screen.getByText("Hà Nội"));
  // Answered: emoji only until the question ends.
  expect(await screen.findByTestId("chat-bar")).toBeTruthy();
  expect(screen.queryByLabelText("Nhắn gì đó lên màn hình…")).toBeNull();

  // The host turns chat off.
  ws.push("game:chat", { mode: "off" });
  await waitFor(() => expect(screen.queryByTestId("chat-bar")).toBeNull());
});

const LAN_JOINED = { playerId: "p1", token: "tok-1", snapshot: { ...lobby, chat: "open", me: { playerId: "p1", nickname: "Lan", avatar: "owl", score: 0, rank: 1, answered: false } } };

async function joinAsLan() {
  mockFetch((url) => (url.endsWith("/games/pin/123456") ? { status: 200, body: { pin: "123456", title: "Địa lý", locked: false } } : undefined));
  renderApp("/join?pin=123456");
  fireEvent.change(await screen.findByLabelText("Biệt danh"), { target: { value: "Lan" } });
  fireEvent.click(screen.getByText("Vào chơi"));
  const ws = await socket();
  ws.ack("player:join", LAN_JOINED);
  return ws;
}

test("a removed player goes back to the name screen to join again", async () => {
  const ws = await joinAsLan();
  ws.push("player:kicked", { banned: false });
  expect(await screen.findByText("Chủ phòng đã mời bạn ra. Bạn có thể vào lại bằng tên khác.")).toBeTruthy();
  expect(await screen.findByLabelText("Biệt danh")).toBeTruthy();
  expect(localStorage.getItem("myquiz.player.123456")).toBeNull();
});

test("a banned player is told and stays out", async () => {
  const ws = await joinAsLan();
  ws.push("player:kicked", { banned: true });
  expect(await screen.findByText("Chủ phòng đã chặn thiết bị này khỏi trò chơi.")).toBeTruthy();
  expect(screen.queryByLabelText("Biệt danh")).toBeNull();
});

test("a muted player sees chat closed until unmuted", async () => {
  const ws = await joinAsLan();
  expect(await screen.findByLabelText("Vỗ tay")).toBeTruthy();
  ws.push("player:muted", { muted: true });
  expect(await screen.findByText("Chủ phòng đã tắt chat của bạn")).toBeTruthy();
  expect(screen.queryByLabelText("Vỗ tay")).toBeNull();
  ws.push("player:muted", { muted: false });
  expect(await screen.findByLabelText("Vỗ tay")).toBeTruthy();
});

test("the host mutes in the lobby and bans mid-game after confirming", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : undefined));
  renderApp("/host/123456");
  const ws = await socket();
  ws.ack("host:attach", { ...lobby, reportId: "s-42", players: [{ id: "p1", nickname: "Lan", avatar: "owl", muted: false }] });

  fireEvent.click(await screen.findByLabelText("Tuỳ chọn cho Lan"));
  fireEvent.click(screen.getByLabelText("Tắt chat của Lan"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:mute", d: { playerId: "p1", muted: true } });
  ws.ack("host:mute", {});
  ws.push("lobby:update", { players: [{ id: "p1", nickname: "Lan", avatar: "owl", muted: true }], count: 1, locked: false });
  expect(await screen.findByLabelText("Đã tắt chat")).toBeTruthy();

  // During a question the player count opens the list.
  ws.push("question:start", question());
  fireEvent.click(await screen.findByLabelText("Quản lý người chơi"));
  expect(screen.getByRole("dialog")).toBeTruthy();
  expect(screen.getByLabelText("Bật chat cho Lan")).toBeTruthy();
  fireEvent.click(screen.getByLabelText("Chặn Lan"));
  expect(ws.sent.at(-1)?.t).not.toBe("host:ban");
  fireEvent.click(screen.getByText("Chặn"));
  expect(ws.sent.at(-1)).toMatchObject({ t: "host:ban", d: { playerId: "p1" } });
});
