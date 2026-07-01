import { useState } from "react";
import QRCode from "qrcode";
import { signup, type SignupBundle } from "../lib/account";

const DEFAULT_SERVER =
  typeof window !== "undefined" ? window.location.origin : "";

export function SignupPage({ onDone }: { onDone: () => void }) {
  const [server, setServer] = useState(DEFAULT_SERVER);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [bundle, setBundle] = useState<SignupBundle | null>(null);
  const [qr, setQR] = useState<string>("");

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const b = await signup(server, email, password);
      setBundle(b);
      setQR(await QRCode.toDataURL(b.setup_code));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  if (bundle) {
    return (
      <section>
        <h2>Save your Emergency Kit</h2>
        <p>
          This is shown <strong>once</strong>. Print it and store it safely. You
          need your password <em>and</em> Secret Key to sign in on a new device.
        </p>
        <pre aria-label="emergency-kit">{bundle.emergency_kit_text}</pre>
        <button onClick={() => window.print()}>Print Emergency Kit</button>
        <h3>Setup Code (add another device)</h3>
        <code aria-label="setup-code">{bundle.setup_code}</code>
        {qr && <img src={qr} alt="setup code QR" width={180} height={180} />}
        <p>
          <button onClick={onDone}>Continue to sign in</button>
        </p>
      </section>
    );
  }

  return (
    <form onSubmit={submit}>
      <h2>Create account</h2>
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
      <button type="submit" disabled={busy}>
        {busy ? "Creating…" : "Sign up"}
      </button>
      {error && <p role="alert">{error}</p>}
    </form>
  );
}
