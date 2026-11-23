import { Route, Routes } from "react-router-dom";
import { RequireAuth } from "./auth/RequireAuth";
import { Layout } from "./components/Layout";
import { Home } from "./pages/Home";
import { Login } from "./pages/Login";
import { Editor } from "./pages/Editor";
import { Placeholder } from "./pages/Placeholder";
import { QuizList } from "./pages/QuizList";

export default function App() {
  return (
    <Routes>
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
        <Route path="join" element={<Placeholder />} />
        <Route path="host/:pin" element={<Placeholder />} />
        <Route path="*" element={<Placeholder />} />
      </Route>
    </Routes>
  );
}
