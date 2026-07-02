import { useEffect, useState } from "react";
import {
  listUsers,
  createInvite,
  setUserEnabled,
  deleteUser,
  getSettings,
  updateSettings,
  setRegistrationPolicy,
  getStats,
  type AdminAuth,
  type AdminUser,
  type InstanceStats,
} from "../lib/admin";

const POLICIES = ["open", "invite-only", "disabled"] as const;

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

/**
 * Admin console: manage accounts (list, invite, enable/disable, delete) and the
 * instance registration policy. Served same-origin, so the API base is "".
 * Admins cannot set passwords (zero-knowledge): "adding a user" mints an invite
 * the user redeems with their own client-computed SRP credentials.
 */
export function AdminPage({
  auth,
  base = "",
  onSignOut,
}: {
  auth: AdminAuth;
  base?: string;
  onSignOut: () => void;
}) {
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [policy, setPolicy] = useState<string>("");
  const [address, setAddress] = useState<string>("");
  const [addressDraft, setAddressDraft] = useState<string>("");
  const [stats, setStats] = useState<InstanceStats | null>(null);
  const [health, setHealth] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [inviteUsername, setInviteUsername] = useState("");
  const [inviteRole, setInviteRole] = useState<"user" | "admin">("user");
  const [invite, setInvite] = useState<string | null>(null);

  async function refresh() {
    setError(null);
    try {
      const [u, s, st] = await Promise.all([
        listUsers(base, auth),
        getSettings(base, auth),
        getStats(base, auth),
      ]);
      setUsers(u);
      setPolicy(s.registration_policy);
      setAddress(s.public_address ?? "");
      setAddressDraft(s.public_address ?? "");
      setStats(st);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
    try {
      const res = await fetch(`${base.replace(/\/+$/, "")}/health`);
      setHealth(res.ok ? "ok" : `error (${res.status})`);
    } catch {
      setHealth("unreachable");
    }
  }

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function changePolicy(next: string) {
    setBusy(true);
    setError(null);
    try {
      setPolicy(await setRegistrationPolicy(base, auth, next));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function saveAddress(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const s = await updateSettings(base, auth, {
        public_address: addressDraft.trim(),
      });
      setAddress(s.public_address ?? "");
      setAddressDraft(s.public_address ?? "");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function mintInvite(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const r = await createInvite(base, auth, {
        username: inviteUsername.trim() || undefined,
        role: inviteRole,
      });
      setInvite(r.invite_token);
      setInviteUsername("");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function toggle(u: AdminUser) {
    setBusy(true);
    setError(null);
    try {
      await setUserEnabled(base, auth, u.id, u.status !== "active");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove(u: AdminUser) {
    if (!confirm(`Delete account "${u.username}"? This cannot be undone.`)) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await deleteUser(base, auth, u.id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <h2>Admin console</h2>
      {error && <p role="alert">{error}</p>}

      <h3>Registration policy</h3>
      <p>
        Controls who may create accounts:{" "}
        <strong>open</strong> (anyone), <strong>invite-only</strong>, or{" "}
        <strong>disabled</strong>.
      </p>
      <div role="group" aria-label="registration policy">
        {POLICIES.map((p) => (
          <label key={p} style={{ marginRight: "1rem" }}>
            <input
              type="radio"
              name="policy"
              value={p}
              checked={policy === p}
              disabled={busy}
              onChange={() => changePolicy(p)}
            />{" "}
            {p}
          </label>
        ))}
      </div>

      <h3>Public server address</h3>
      <p style={{ opacity: 0.75 }}>
        The base URL clients should use to reach this instance. Shown to users
        setting up new devices; leave blank to unset.
      </p>
      <form onSubmit={saveAddress}>
        <label>
          Address
          <input
            value={addressDraft}
            onChange={(e) => setAddressDraft(e.target.value)}
            placeholder="https://tock.example.com"
          />
        </label>
        <button type="submit" disabled={busy || addressDraft.trim() === address}>
          Save
        </button>
      </form>

      <h3>Usage &amp; health</h3>
      <p>
        Server health:{" "}
        <strong>{health ?? "checking…"}</strong>
      </p>
      {stats && (
        <table>
          <tbody>
            <tr>
              <td>Accounts</td>
              <td>
                {stats.accounts_total} ({stats.accounts_active} active,{" "}
                {stats.accounts_disabled} disabled, {stats.accounts_admin} admin)
              </td>
            </tr>
            <tr>
              <td>Vaults</td>
              <td>{stats.vaults}</td>
            </tr>
            <tr>
              <td>Devices</td>
              <td>{stats.devices}</td>
            </tr>
            <tr>
              <td>Events</td>
              <td>{stats.events}</td>
            </tr>
            <tr>
              <td>Encrypted storage</td>
              <td>{formatBytes(stats.storage_bytes)}</td>
            </tr>
          </tbody>
        </table>
      )}

      <h3>Invite a user</h3>
      <form onSubmit={mintInvite}>
        <label>
          Username (optional)
          <input
            value={inviteUsername}
            onChange={(e) => setInviteUsername(e.target.value)}
          />
        </label>
        <label>
          Role
          <select
            value={inviteRole}
            onChange={(e) => setInviteRole(e.target.value as "user" | "admin")}
          >
            <option value="user">user</option>
            <option value="admin">admin</option>
          </select>
        </label>
        <button type="submit" disabled={busy}>
          Create invite
        </button>
      </form>
      {invite && (
        <p role="status">
          Invite token (hand it to the user to register):{" "}
          <code aria-label="invite-token">{invite}</code>
        </p>
      )}

      <h3>Users ({users.length})</h3>
      <button onClick={() => void refresh()} disabled={busy}>
        Refresh
      </button>
      <table>
        <thead>
          <tr>
            <th>Username</th>
            <th>Role</th>
            <th>Status</th>
            <th>Created</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {users.map((u) => (
            <tr key={u.id}>
              <td>{u.username}</td>
              <td>{u.role}</td>
              <td>{u.status}</td>
              <td>{u.created_at}</td>
              <td>
                <button onClick={() => void toggle(u)} disabled={busy}>
                  {u.status === "active" ? "Disable" : "Enable"}
                </button>{" "}
                <button onClick={() => void remove(u)} disabled={busy}>
                  Delete
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <p>
        <button onClick={onSignOut}>Sign out</button>
      </p>
    </section>
  );
}
