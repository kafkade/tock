import { useCallback, useEffect, useState } from "react";
import QRCode from "qrcode";
import type { Session } from "../lib/account";
import {
  buildSetupCode,
  listDevices,
  listSessions,
  revokeDevice,
  revokeOtherSessions,
  revokeSession,
  rotatePassword,
  sessionAuth,
  type DeviceItem,
  type SessionItem,
} from "../lib/selfservice";

/**
 * Self-service account portal (issue #131). Lets a signed-in user manage their
 * own devices and sessions, re-display their add-device Setup Code, and rotate
 * their password. The Secret Key is held only in memory (never persisted) for
 * the lifetime of the session, so Setup-Code regeneration and rotation work
 * without asking the user to paste it again.
 */
export function AccountPage({
  base,
  serverURL,
  email,
  session,
  secretKey,
  onBack,
  onSignOut,
}: {
  base: string;
  serverURL: string;
  email: string;
  session: Session;
  secretKey: string | null;
  onBack: () => void;
  onSignOut: () => void;
}) {
  const auth = sessionAuth(session);

  const [devices, setDevices] = useState<DeviceItem[] | null>(null);
  const [sessions, setSessions] = useState<SessionItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [d, s] = await Promise.all([
        listDevices(base, auth),
        listSessions(base, auth),
      ]);
      setDevices(d);
      setSessions(s);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
    // auth is derived from the immutable session; refetch only on base/session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [base, session]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function onRevokeDevice(id: string) {
    setError(null);
    try {
      await revokeDevice(base, auth, id);
      setNotice("Device revoked.");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function onRevokeSession(id: string, current: boolean) {
    setError(null);
    try {
      await revokeSession(base, auth, id);
      if (current) {
        onSignOut();
        return;
      }
      setNotice("Session revoked.");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function onRevokeOthers() {
    setError(null);
    try {
      const n = await revokeOtherSessions(base, auth);
      setNotice(`Revoked ${n} other session(s).`);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <section>
      <div style={{ display: "flex", justifyContent: "space-between" }}>
        <h2>Your account</h2>
        <button onClick={onBack}>← Back</button>
      </div>
      <p style={{ opacity: 0.75 }}>{email}</p>

      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}

      <RotatePassword
        base={base}
        auth={auth}
        secretKey={secretKey}
        onDone={(msg) => {
          setNotice(msg);
          void refresh();
        }}
        onError={setError}
      />

      <SetupCodePanel
        serverURL={serverURL}
        email={email}
        secretKey={secretKey}
      />

      <h3>Devices</h3>
      {devices === null ? (
        <p role="status">Loading devices…</p>
      ) : devices.length === 0 ? (
        <p>No devices registered.</p>
      ) : (
        <ul>
          {devices.map((d) => (
            <li key={d.id}>
              <code>{d.label ?? d.id}</code>{" "}
              <span style={{ opacity: 0.7 }}>
                · registered {new Date(d.registered_at).toLocaleString()}
                {d.revoked ? " · revoked" : ""}
              </span>{" "}
              {!d.revoked && (
                <button onClick={() => void onRevokeDevice(d.id)}>Revoke</button>
              )}
            </li>
          ))}
        </ul>
      )}

      <h3>Sessions</h3>
      {sessions === null ? (
        <p role="status">Loading sessions…</p>
      ) : (
        <>
          <ul>
            {sessions.map((s) => (
              <li key={s.id}>
                <code>{s.id.slice(0, 12)}…</code>{" "}
                <span style={{ opacity: 0.7 }}>
                  · started {new Date(s.created_at).toLocaleString()} · expires{" "}
                  {new Date(s.expires_at * 1000).toLocaleString()}
                </span>
                {s.current && <strong> · this device</strong>}{" "}
                <button onClick={() => void onRevokeSession(s.id, s.current)}>
                  {s.current ? "Sign out" : "Revoke"}
                </button>
              </li>
            ))}
          </ul>
          {sessions.some((s) => !s.current) && (
            <button onClick={() => void onRevokeOthers()}>
              Sign out all other sessions
            </button>
          )}
        </>
      )}

      <p style={{ marginTop: "2rem" }}>
        <button onClick={onSignOut}>Sign out</button>
      </p>
    </section>
  );
}

function RotatePassword({
  base,
  auth,
  secretKey,
  onDone,
  onError,
}: {
  base: string;
  auth: ReturnType<typeof sessionAuth>;
  secretKey: string | null;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  const disabled = !secretKey;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!secretKey) {
      onError("Your Secret Key isn’t available — sign in again to rotate.");
      return;
    }
    if (newPw !== confirm) {
      onError("New password and confirmation don’t match.");
      return;
    }
    setBusy(true);
    try {
      await rotatePassword(base, auth, oldPw, newPw, secretKey);
      setOldPw("");
      setNewPw("");
      setConfirm("");
      onDone(
        "Password rotated. Other devices must sign in again with the new password.",
      );
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit}>
      <h3>Change password</h3>
      {disabled && (
        <p role="note" style={{ opacity: 0.75 }}>
          Your Secret Key isn’t in memory (you may have reloaded). Sign in again
          to change your password.
        </p>
      )}
      <label>
        Current password
        <input
          type="password"
          value={oldPw}
          disabled={disabled}
          onChange={(e) => setOldPw(e.target.value)}
        />
      </label>
      <label>
        New password
        <input
          type="password"
          value={newPw}
          disabled={disabled}
          onChange={(e) => setNewPw(e.target.value)}
        />
      </label>
      <label>
        Confirm new password
        <input
          type="password"
          value={confirm}
          disabled={disabled}
          onChange={(e) => setConfirm(e.target.value)}
        />
      </label>
      <button type="submit" disabled={disabled || busy || !oldPw || !newPw}>
        {busy ? "Rotating…" : "Change password"}
      </button>
    </form>
  );
}

function SetupCodePanel({
  serverURL,
  email,
  secretKey,
}: {
  serverURL: string;
  email: string;
  secretKey: string | null;
}) {
  const [code, setCode] = useState<string | null>(null);
  const [qr, setQR] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function reveal() {
    if (!secretKey) {
      setError("Your Secret Key isn’t available — sign in again to regenerate.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const c = await buildSetupCode(serverURL, email, secretKey);
      setCode(c);
      setQR(await QRCode.toDataURL(c));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  function hide() {
    setCode(null);
    setQR("");
  }

  return (
    <section>
      <h3>Add another device</h3>
      <p style={{ opacity: 0.75 }}>
        The Setup Code bundles this server, your email, and your Secret Key so a
        new device can sign in with just your password. Treat it like a
        password — anyone who has it (and your password) can access your account.
      </p>
      {error && <p role="alert">{error}</p>}
      {code === null ? (
        <button onClick={() => void reveal()} disabled={busy || !secretKey}>
          {busy ? "Generating…" : "Reveal Setup Code"}
        </button>
      ) : (
        <div>
          <code aria-label="setup-code">{code}</code>
          {qr && <img src={qr} alt="setup code QR" width={180} height={180} />}
          <p>
            <button onClick={() => window.print()}>Print</button>{" "}
            <button onClick={hide}>Hide</button>
          </p>
        </div>
      )}
    </section>
  );
}
