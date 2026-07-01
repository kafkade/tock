import { useEffect, useState } from "react";
import { SignupPage } from "./pages/SignupPage";
import { LoginPage } from "./pages/LoginPage";
import { TasksPage } from "./pages/TasksPage";
import { SetupPage } from "./pages/SetupPage";
import { AdminPage } from "./pages/AdminPage";
import { MemoryStore, type StoredCredentials } from "./lib/credentials";
import type { Session } from "./lib/account";
import {
  fetchServerInfo,
  isAdmin,
  type AdminAuth,
  type ServerInfo,
} from "./lib/admin";

type View = "loading" | "setup" | "login" | "signup" | "tasks" | "admin";

// The console is served same-origin behind the reverse proxy, so the API base
// is relative (""). fetchServerInfo/admin calls resolve against this origin.
const BASE = "";

export function App() {
  const [store] = useState(() => new MemoryStore());
  const [view, setView] = useState<View>("loading");
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [creds, setCreds] = useState<StoredCredentials | null>(null);
  const [admin, setAdmin] = useState<{ auth: AdminAuth; base: string } | null>(
    null,
  );

  async function loadInfo() {
    setLoadError(null);
    try {
      const i = await fetchServerInfo(BASE);
      setInfo(i);
      setView(i.setup_required ? "setup" : "login");
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : String(err));
      setView("login");
    }
  }

  useEffect(() => {
    void loadInfo();
  }, []);

  function enterAdmin(auth: AdminAuth, base: string) {
    setAdmin({ auth, base });
    setView("admin");
  }

  async function loggedIn(serverURL: string, email: string, session: Session) {
    const auth: AdminAuth = {
      bearerToken: session.bearer_token,
      channelBinding: session.channel_binding,
    };
    // An admin session lands in the console; everyone else in the task view.
    let adminSession = false;
    try {
      adminSession = await isAdmin(serverURL, auth);
    } catch {
      adminSession = false;
    }
    if (adminSession) {
      enterAdmin(auth, serverURL);
      return;
    }
    const c = { serverURL, email, session };
    store.save(c);
    setCreds(c);
    setView("tasks");
  }

  function signOut() {
    store.clear();
    setCreds(null);
    setAdmin(null);
    setView(info?.setup_required ? "setup" : "login");
  }

  return (
    <main style={{ maxWidth: 720, margin: "2rem auto", fontFamily: "system-ui" }}>
      <h1>tock</h1>

      {view === "loading" && <p role="status">Checking instance…</p>}

      {loadError && view !== "loading" && (
        <p role="alert">
          Couldn’t reach the server ({loadError}).{" "}
          <button onClick={() => void loadInfo()}>Retry</button>
        </p>
      )}

      {view === "setup" && info && (
        <SetupPage
          info={info}
          base={BASE}
          onReady={(auth) => enterAdmin(auth, BASE)}
        />
      )}

      {view === "admin" && admin && (
        <AdminPage auth={admin.auth} base={admin.base} onSignOut={signOut} />
      )}

      {view === "tasks" && creds && (
        <TasksPage creds={creds} onLogout={signOut} />
      )}

      {view === "signup" && <SignupPage onDone={() => setView("login")} />}

      {view === "login" && (
        <>
          <LoginPage onLoggedIn={loggedIn} />
          <p>
            No account?{" "}
            <button onClick={() => setView("signup")}>Sign up</button>
          </p>
        </>
      )}
    </main>
  );
}
