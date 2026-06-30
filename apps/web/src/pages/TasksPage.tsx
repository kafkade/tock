import { useState } from "react";
import type { StoredCredentials } from "../lib/credentials";

export function TasksPage({
  creds,
  onLogout,
}: {
  creds: StoredCredentials;
  onLogout: () => void;
}) {
  const [vaultID, setVaultID] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  async function checkAuth() {
    setStatus("Checking…");
    try {
      const { pullEventCount } = await import("../lib/sync");
      const n = await pullEventCount(creds.serverURL, vaultID, creds.session);
      setStatus(`Authenticated. Server returned ${n} event(s) (ciphertext).`);
    } catch (err) {
      setStatus(err instanceof Error ? err.message : String(err));
    }
  }

  const expires = new Date(creds.session.expires_at * 1000).toLocaleString();

  return (
    <section>
      <h2>Signed in as {creds.email}</h2>
      <p>Session expires: {expires}</p>
      <p>
        Full task editing needs a local encrypted vault in the browser (a WASM
        SQLite vault), which lands in a follow-up. For now this verifies the
        authenticated, channel-bound sync round-trip.
      </p>
      <label>
        Vault ID
        <input value={vaultID} onChange={(e) => setVaultID(e.target.value)} />
      </label>
      <button onClick={checkAuth} disabled={!vaultID}>
        Verify authed sync
      </button>
      {status && <p role="status">{status}</p>}
      <p>
        <button onClick={onLogout}>Sign out</button>
      </p>
    </section>
  );
}
