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

const lobby = { pin: "123456", state: { state: "lobby" }, locked: false, players: [], question: null, me: null };
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

  ws.push("question:result", { index: 0, correct: true, points: 950, total: 950, rank: 1, correctOptionIds: [0] });
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

test("a wrong PIN is explained", async () => {
  mockFetch(() => undefined);
  renderApp("/join?pin=999999");
  expect(await screen.findByText("Mã PIN không đúng hoặc trò chơi đã kết thúc.")).toBeTruthy();
});

test("the host runs the lobby, starts, and moves on after the reveal", async () => {
  mockFetch((url) => (url.endsWith("/me") ? { status: 200, body: ME } : undefined));
  renderApp("/host/123456");
  const ws = await socket();
  expect(ws.sent[0]).toMatchObject({ t: "host:attach", d: { pin: "123456" } });
  ws.ack("host:attach", { ...lobby, players: [{ id: "p1", nickname: "Lan" }] });
  expect(await screen.findByText("123456")).toBeTruthy();
  expect(screen.getByText("1 người chơi")).toBeTruthy();

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
});
