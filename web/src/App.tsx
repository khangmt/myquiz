import { Route, Routes } from "react-router-dom";
import { RequireAuth } from "./auth/RequireAuth";
import { Layout } from "./components/Layout";
import { Home } from "./pages/Home";
import { Login } from "./pages/Login";
import { Editor } from "./pages/Editor";
import { Placeholder } from "./pages/Placeholder";
import { Host } from "./game/Host";
import { Play } from "./game/Play";
import { MediaWindow } from "./media/MediaWindow";
import { Report } from "./reports/Report";
import { Reports } from "./reports/Reports";
import { QuizList } from "./pages/QuizList";
import { Admin } from "./admin/Admin";

export default function App() {
  return (
    <Routes>
      {/* The live game has its own full-screen stage, without the site header. */}
      <Route path="join" element={<Play />} />
      <Route
        path="host/:pin"
        element={
          <RequireAuth>
            <Host />
          </RequireAuth>
        }
      />
      {/* The host's pop-out window for a question's image or code. */}
      <Route
        path="host/:pin/media"
        element={
          <RequireAuth>
            <MediaWindow />
          </RequireAuth>
        }
      />
      <Route element={<Layout />}>
        <Route index element={<Home />} />
        <Route path="login" element={<Login />} />
        <Route
          path="quizzes"
          element={
            <RequireAuth>
              <QuizList />
            </RequireAuth>
          }
        />
        <Route
          path="quizzes/:id/edit"
          element={
            <RequireAuth>
              <Editor />
            </RequireAuth>
          }
        />
        <Route
          path="reports"
          element={
            <RequireAuth>
              <Reports />
            </RequireAuth>
          }
        />
        <Route
          path="reports/:id"
          element={
            <RequireAuth>
              <Report />
            </RequireAuth>
          }
        />
        <Route
          path="admin"
          element={
            <RequireAuth>
              <Admin />
            </RequireAuth>
          }
        />
        <Route path="*" element={<Placeholder />} />
      </Route>
    </Routes>
  );
}
