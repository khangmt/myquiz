import { render, screen, fireEvent } from "@testing-library/react";
import "./i18n";
import App from "./App";

test("shows Vietnamese by default and switches to English", () => {
  render(<App />);
  expect(screen.getByText("Tham gia trò chơi")).toBeTruthy();
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "en" } });
  expect(screen.getByText("Join a game")).toBeTruthy();
});

test("PIN field keeps digits only", () => {
  render(<App />);
  const input = screen.getAllByLabelText(/PIN/)[0] as HTMLInputElement;
  fireEvent.change(input, { target: { value: "12a3b4" } });
  expect(input.value).toBe("1234");
});
