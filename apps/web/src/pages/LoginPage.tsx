import { useState } from "react";
import { login, parseSetupCode, type Session } from "../lib/account";

const DEFAULT_SERVER = "http://localhost:8787";

export function LoginPage({
  onLoggedIn,
}: {
  onLoggedIn: (server: string, email: string, session: Session) => void;
}) {
  const [server, setServer] = useState(DEFAULT_SERVER);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function pasteSetupCode() {
    setError(null);
    try {
      const parsed = await parseSetupCode(secretKey.trim());
      setServer(parsed.server_url);
      setEmail(parsed.email);
      setSecretKey(parsed.secret_key);
    } catch {
      setError("Not a valid TOCK1 Setup Code.");
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const session = await login(server, email, password, secretKey.trim());
      onLoggedIn(server, email, session);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit}>
      <h2>Sign in</h2>
      <label>
        Server
        <input value={server} onChange={(e) => setServer(e.target.value)} />
      </label>
      <label>
        Email
        <input value={email} onChange={(e) => setEmail(e.target.value)} />
      </label>
      <label>
        Password
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </label>
      <label>
        Secret Key or Setup Code
        <input
          value={secretKey}
          onChange={(e) => setSecretKey(e.target.value)}
          placeholder="A4-… or TOCK1:…"
        />
      </label>
      <button type="button" onClick={pasteSetupCode}>
        Decode Setup Code
      </button>
      <button type="submit" disabled={busy}>
        {busy ? "Signing in…" : "Sign in"}
      </button>
      {error && <p role="alert">{error}</p>}
    </form>
  );
}
