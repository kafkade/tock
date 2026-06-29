import { useMemo, useState } from "react";
import { SignupPage } from "./pages/SignupPage";
import { LoginPage } from "./pages/LoginPage";
import { TasksPage } from "./pages/TasksPage";
import { MemoryStore, type StoredCredentials } from "./lib/credentials";
import type { Session } from "./lib/account";

type View = "login" | "signup" | "tasks";

export function App() {
  const store = useMemo(() => new MemoryStore(), []);
  const [view, setView] = useState<View>("login");
  const [creds, setCreds] = useState<StoredCredentials | null>(store.load());

  function loggedIn(serverURL: string, email: string, session: Session) {
    const c = { serverURL, email, session };
    store.save(c);
    setCreds(c);
    setView("tasks");
  }

  function logout() {
    store.clear();
    setCreds(null);
    setView("login");
  }

  return (
    <main style={{ maxWidth: 560, margin: "2rem auto", fontFamily: "system-ui" }}>
      <h1>tock</h1>
      {view === "tasks" && creds ? (
        <TasksPage creds={creds} onLogout={logout} />
      ) : view === "signup" ? (
        <SignupPage onDone={() => setView("login")} />
      ) : (
        <>
          <LoginPage onLoggedIn={loggedIn} />
          <p>
            No account? <button onClick={() => setView("signup")}>Sign up</button>
          </p>
        </>
      )}
    </main>
  );
}
