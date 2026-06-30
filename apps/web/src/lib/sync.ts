// Minimal authed sync smoke: register the absence of a browser vault honestly.
// The web client has no local SQLite vault yet, so it cannot decrypt events;
// this verifies the bearer + channel-binding auth round-trips against the
// server's ciphertext-only event store.

import { authHeaders, type Session } from "./account";

export async function pullEventCount(
  serverURL: string,
  vaultID: string,
  session: Session,
): Promise<number> {
  const base = serverURL.replace(/\/+$/, "");
  const id = vaultID.replace(/-/g, "").toLowerCase();
  const res = await fetch(`${base}/v1/vaults/${id}/events/pull?after=0&limit=1`, {
    headers: authHeaders(session),
  });
  if (!res.ok) throw new Error(`pull failed (${res.status})`);
  const body = (await res.json()) as { events: unknown[] };
  return body.events.length;
}
