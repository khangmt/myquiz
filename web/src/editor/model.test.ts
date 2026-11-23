import { move, newQuestion, toggleCorrect, toSave, withKind } from "./model";

const labels = { true: "Đúng", false: "Sai" };

test("new questions follow the type defaults", () => {
  expect(newQuestion("single", labels).options).toHaveLength(2);
  expect(newQuestion("single", labels).timeLimitS).toBe(20);
  const fill = newQuestion("fill_blank", labels);
  expect(fill.timeLimitS).toBe(30);
  expect(fill.text).toBe("___");
  expect(fill.options).toEqual([]);
  expect(newQuestion("true_false", labels).options).toEqual([
    { text: "Đúng", correct: true },
    { text: "Sai", correct: false },
  ]);
});

test("fill-in-the-blank converts to multiple choice with its answers correct (FR-QZ-05)", () => {
  const fill = { ...newQuestion("fill_blank", labels), text: "Thủ đô là ___", acceptedAnswers: ["Hà Nội", "Ha Noi"] };
  const multi = withKind(fill, "multi", labels);
  expect(multi.options).toEqual([
    { text: "Hà Nội", correct: true },
    { text: "Ha Noi", correct: true },
  ]);
  expect(multi.acceptedAnswers).toEqual([]);
  // Single keeps only the first correct one.
  expect(withKind(fill, "single", labels).options.map((o) => o.correct)).toEqual([true, false]);
  // And back: correct options become accepted answers.
  const back = withKind(multi, "fill_blank", labels);
  expect(back.acceptedAnswers).toEqual(["Hà Nội", "Ha Noi"]);
  expect(back.text).toBe("Thủ đô là ___");
});

test("single allows one correct option; multi toggles", () => {
  const q = { ...newQuestion("single", labels), options: [{ text: "a", correct: true }, { text: "b", correct: false }] };
  expect(toggleCorrect(q, 1).options.map((o) => o.correct)).toEqual([false, true]);
  const m = { ...q, type: "multi" as const };
  expect(toggleCorrect(m, 1).options.map((o) => o.correct)).toEqual([true, true]);
  expect(toggleCorrect(m, 0).options.map((o) => o.correct)).toEqual([false, false]);
});

test("move reorders and ignores out-of-range moves", () => {
  expect(move([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
  expect(move([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
});

test("the save body drops display-only fields", () => {
  const q = { ...newQuestion("single", labels), mediaId: "m1", mediaUrl: "/media/x.webp" };
  const body = toSave({ title: "T", description: "", coverMediaId: null, coverMediaUrl: "/media/c.webp", visibility: "private", questions: [q] });
  expect(body).not.toHaveProperty("coverMediaUrl");
  expect(body.questions[0].mediaUrl).toBeNull();
  expect(body.questions[0].mediaId).toBe("m1");
});
