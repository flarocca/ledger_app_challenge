"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { api } from "@/lib/api";

type LoginResp = {
  user_id: number;
  username: string;
  session_id: string;
};

export default function LoginPage() {
  const router = useRouter();
  const [username, setUsername] = useState("alice");
  const [password, setPassword] = useState("password123");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api<LoginResp>("/auth/login", {
        method: "POST",
        body: { username, password },
      });
      router.replace("/home");
    } catch (err) {
      setError((err as Error).message || "Login failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="shell">
      <div className="panel">
        <div className="h1">Sign in</div>
        <div className="muted" style={{ marginBottom: 16 }}>
          Seeded users: alice / bob / carol / dave — password{" "}
          <code>password123</code>
        </div>
        <form onSubmit={submit}>
          <div className="field">
            <label className="label">Username</label>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
            />
          </div>
          <div className="field">
            <label className="label">Password</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <button type="submit" disabled={busy}>
            {busy ? "Signing in…" : "Sign in"}
          </button>
          {error && <div className="error">{error}</div>}
        </form>
      </div>
    </div>
  );
}
