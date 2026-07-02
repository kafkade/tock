import { useState } from "react";
import QRCode from "qrcode";
import {
  signupFirstAdmin,
  updateSettings,
  type AdminAuth,
  type ServerInfo,
} from "../lib/admin";
import type { SignupBundle } from "../lib/account";

const DEFAULT_ADDRESS =
  typeof window !== "undefined" ? window.location.origin : "";

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
  const [policy, setPolicy] = useState("invite-only");
  const [address, setAddress] = useState(DEFAULT_ADDRESS);
  const [finishing, setFinishing] = useState(false);

  async function finish() {
    if (!auth) return;
    setFinishing(true);
    setError(null);
    try {
      await updateSettings(base, auth, {
        registration_policy: policy,
        public_address: address.trim(),
      });
      onReady(auth);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setFinishing(false);
    }
  }

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

        <h3>Instance settings</h3>
        <p style={{ opacity: 0.75 }}>
          Choose who can register and the public address clients should use to
          reach this server. You can change both later in the console.
        </p>
        <label>
          Registration policy
          <select value={policy} onChange={(e) => setPolicy(e.target.value)}>
            <option value="invite-only">Invite only</option>
            <option value="open">Open</option>
            <option value="disabled">Disabled</option>
          </select>
        </label>
        <label>
          Public server address
          <input
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            placeholder="https://tock.example.com"
          />
        </label>

        <button disabled={!saved || finishing} onClick={() => void finish()}>
          {finishing ? "Saving…" : "Continue to the admin console"}
        </button>
        {error && <p role="alert">{error}</p>}
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
