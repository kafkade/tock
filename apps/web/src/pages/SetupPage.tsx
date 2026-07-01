import { useState } from "react";
import QRCode from "qrcode";
import { signupFirstAdmin, type AdminAuth, type ServerInfo } from "../lib/admin";
import type { SignupBundle } from "../lib/account";

/**
 * First-run setup wizard. Shown when the instance reports `setup_required`.
 * Creates the admin account (the first registrant is bootstrapped as admin),
 * then forces the operator to save their Emergency Kit before entering the
 * admin console. The console is served same-origin, so the API base is "".
 */
export function SetupPage({
  info,
  base = "",
  onReady,
}: {
  info: ServerInfo;
  base?: string;
  onReady: (auth: AdminAuth) => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [bundle, setBundle] = useState<SignupBundle | null>(null);
  const [auth, setAuth] = useState<AdminAuth | null>(null);
  const [qr, setQR] = useState<string>("");
  const [saved, setSaved] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await signupFirstAdmin(base, email, password);
      setBundle(result.bundle);
      setAuth(result.auth);
      setQR(await QRCode.toDataURL(result.bundle.setup_code));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  if (bundle && auth) {
    return (
      <section>
        <h2>Save your Emergency Kit</h2>
        <p>
          This is shown <strong>once</strong>. Print it and store it somewhere
          safe. You need your password <em>and</em> Secret Key to sign in on a
          new device — nobody, including this server, can recover them for you.
        </p>
        <pre aria-label="emergency-kit">{bundle.emergency_kit_text}</pre>
        <button onClick={() => window.print()}>Print Emergency Kit</button>
        <h3>Setup Code (to add another device)</h3>
        <code aria-label="setup-code">{bundle.setup_code}</code>
        {qr && <img src={qr} alt="setup code QR" width={180} height={180} />}
        <p>
          <label>
            <input
              type="checkbox"
              checked={saved}
              onChange={(e) => setSaved(e.target.checked)}
            />{" "}
            I have saved my Emergency Kit
          </label>
        </p>
        <button disabled={!saved} onClick={() => onReady(auth)}>
          Continue to the admin console
        </button>
      </section>
    );
  }

  return (
    <form onSubmit={submit}>
      <h2>Welcome — set up your tock instance</h2>
      <p>
        No account exists yet, so this first account becomes the{" "}
        <strong>administrator</strong>. Everything is end-to-end encrypted; the
        server only ever stores ciphertext.
      </p>
      <p style={{ fontSize: "0.85em", opacity: 0.75 }}>
        {info.mode} · v{info.version}
      </p>
      <label>
        Admin email
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
      <button type="submit" disabled={busy || !email || !password}>
        {busy ? "Creating…" : "Create admin account"}
      </button>
      {error && <p role="alert">{error}</p>}
    </form>
  );
}
