"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  api,
  ApiError,
  FeedItem,
  formatMoney,
  MeResponse,
  newRequestId,
  TransferRecipientInput,
  TransferResult,
} from "@/lib/api";
import { openFeedStream, FeedEvent } from "@/lib/feedStream";
import { isRecentDuplicate, recordTransfer } from "@/lib/recentTransfers";

type RecipientRow = { username: string; amount: string };

const emptyRow = (): RecipientRow => ({ username: "", amount: "" });

export default function HomePage() {
  const router = useRouter();
  const [me, setMe] = useState<MeResponse | null>(null);
  const [items, setItems] = useState<FeedItem[]>([]);
  const [highlighted, setHighlighted] = useState<number | null>(null);
  const [rows, setRows] = useState<RecipientRow[]>([emptyRow()]);
  const [requestId, setRequestId] = useState<string>(() => newRequestId());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [duplicateWarning, setDuplicateWarning] = useState<string | null>(null);
  const seen = useRef<Set<number>>(new Set());
  const inFlight = useRef(false);

  const loadMe = useCallback(async () => {
    try {
      const data = await api<MeResponse>("/users/me");
      setMe(data);
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) router.replace("/login");
    }
  }, [router]);

  const loadFeed = useCallback(async () => {
    try {
      const data = await api<{ items: FeedItem[] }>("/feed");
      setItems(data.items);
      seen.current = new Set(data.items.map((it) => it.id));
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) router.replace("/login");
    }
  }, [router]);

  useEffect(() => {
    void loadMe();
    void loadFeed();
  }, [loadMe, loadFeed]);

  useEffect(() => {
    const close = openFeedStream((ev: FeedEvent) => {
      if (seen.current.has(ev.id)) return;
      seen.current.add(ev.id);
      setItems((prev) => [
        {
          id: ev.id,
          operation_id: ev.operation_id,
          sender_username: ev.sender_username,
          recipient_username: ev.recipient_username,
          amount: ev.amount,
          currency: ev.currency,
          created_at: ev.created_at,
        },
        ...prev,
      ]);
      setHighlighted(ev.id);
      void loadMe();
    });
    return close;
  }, [loadMe]);

  function updateRow(idx: number, patch: Partial<RecipientRow>) {
    setDuplicateWarning(null);
    setRows((prev) => prev.map((r, i) => (i === idx ? { ...r, ...patch } : r)));
  }

  function addRow() {
    setDuplicateWarning(null);
    setRows((prev) => [...prev, emptyRow()]);
  }

  function removeRow(idx: number) {
    setDuplicateWarning(null);
    setRows((prev) => (prev.length === 1 ? prev : prev.filter((_, i) => i !== idx)));
  }

  async function send(e: React.FormEvent) {
    e.preventDefault();
    // Synchronous guard against rapid double-clicks / Enter-key resubmits:
    // React commits `setBusy(true)` on the next render, so the `disabled` prop
    // on the submit button isn't in the DOM yet when the second event fires.
    if (inFlight.current) return;

    const recipients: TransferRecipientInput[] = rows.map((r) => ({
      recipient_username: r.username.trim(),
      amount: r.amount.trim(),
    }));

    if (recipients.some((r) => !r.recipient_username || !r.amount)) {
      setError("Every recipient needs a username and an amount");
      return;
    }
    const usernames = recipients.map((r) => r.recipient_username);
    if (new Set(usernames).size !== usernames.length) {
      setError("Each recipient can only appear once per transfer");
      return;
    }

    const currency = "USD";

    // Warn once if the exact same transfer went out in the last 5 minutes.
    // A repeat click on the button after the warning shows counts as
    // acknowledgement and sends through.
    if (!duplicateWarning && isRecentDuplicate(recipients, currency)) {
      setError(null);
      setInfo(null);
      setDuplicateWarning(
        "This looks like the same transfer you just sent. Click \"Send anyway\" to confirm.",
      );
      return;
    }

    inFlight.current = true;
    setBusy(true);
    setError(null);
    setInfo(null);

    try {
      const result = await api<TransferResult>("/transfers", {
        method: "POST",
        idempotencyKey: requestId,
        body: { recipients, currency },
      });
      recordTransfer(recipients, currency);
      const summary = result.transfers
        .map((t) => `${formatMoney(t.amount, result.currency)} → ${t.recipient_username}`)
        .join(", ");
      setInfo(`Sent ${summary}`);
      setRows([emptyRow()]);
      setRequestId(newRequestId());
      setDuplicateWarning(null);
      void loadMe();
    } catch (err) {
      setError((err as Error).message || "Transfer failed");
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  async function logout() {
    try {
      await api("/auth/logout", { method: "POST" });
    } catch {
      // ignore
    }
    router.replace("/login");
  }

  return (
    <div className="shell">
      <div className="panel">
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <div>
            <div className="muted">Signed in as</div>
            <div className="h1">{me?.username || "…"}</div>
            <div className="balance">
              {me ? formatMoney(me.balance, me.currency) : "—"}
            </div>
          </div>
          <button
            type="button"
            className="linkish"
            onClick={logout}
            style={{ padding: "8px 12px" }}
          >
            Log out
          </button>
        </div>
      </div>

      <div className="panel">
        <div className="h1">Send money</div>
        <form onSubmit={send}>
          {rows.map((row, idx) => (
            <div className="row" key={idx}>
              <div className="field" style={{ flex: 2 }}>
                <label className="label">Recipient username</label>
                <input
                  type="text"
                  value={row.username}
                  onChange={(e) => updateRow(idx, { username: e.target.value })}
                  placeholder="e.g. bob"
                />
              </div>
              <div className="field" style={{ flex: 1 }}>
                <label className="label">Amount (USD)</label>
                <input
                  type="text"
                  inputMode="decimal"
                  value={row.amount}
                  onChange={(e) => updateRow(idx, { amount: e.target.value })}
                  placeholder="0.00"
                />
              </div>
              {rows.length > 1 && (
                <button
                  type="button"
                  className="linkish"
                  onClick={() => removeRow(idx)}
                  aria-label={`Remove recipient ${idx + 1}`}
                  style={{ alignSelf: "flex-end" }}
                >
                  Remove
                </button>
              )}
            </div>
          ))}
          <div className="row">
            <button type="button" className="linkish" onClick={addRow}>
              + Add recipient
            </button>
          </div>
          <button type="submit" disabled={busy}>
            {busy
              ? "Sending…"
              : duplicateWarning
                ? "Send anyway"
                : rows.length > 1
                  ? `Send to ${rows.length} recipients`
                  : "Send"}
          </button>
          {duplicateWarning && <div className="warning">{duplicateWarning}</div>}
          {error && <div className="error">{error}</div>}
          {info && <div className="success">{info}</div>}
        </form>
      </div>

      <div className="panel">
        <div className="h1">Global feed</div>
        {items.length === 0 && (
          <div className="muted">No transfers yet. Send one!</div>
        )}
        {items.map((it) => (
          <div
            key={it.id}
            className={`feed-item${highlighted === it.id ? " new" : ""}`}
          >
            <div>
              <strong>{it.sender_username}</strong>
              <span className="muted"> → </span>
              <strong>{it.recipient_username}</strong>
            </div>
            <div>{formatMoney(it.amount, it.currency)}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
