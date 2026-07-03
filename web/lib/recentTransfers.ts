import type { TransferRecipientInput } from "./api";

const STORAGE_KEY = "recentTransfers.v1";
const WINDOW_MS = 5 * 60 * 1000;

type Entry = {
  fingerprint: string;
  timestamp: number;
};

function fingerprint(
  recipients: TransferRecipientInput[],
  currency: string,
): string {
  // Sort so recipient order doesn't matter; normalize username case and treat
  // "25" and "25.00" as the same amount.
  const normalized = recipients
    .map((r) => ({
      recipient_username: r.recipient_username.trim().toLowerCase(),
      amount: Number(r.amount),
    }))
    .sort((a, b) =>
      a.recipient_username.localeCompare(b.recipient_username),
    );
  return JSON.stringify({ currency, recipients: normalized });
}

function load(): Entry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Entry[];
    if (!Array.isArray(parsed)) return [];
    const now = Date.now();
    return parsed.filter(
      (e) =>
        e &&
        typeof e.fingerprint === "string" &&
        typeof e.timestamp === "number" &&
        now - e.timestamp < WINDOW_MS,
    );
  } catch {
    return [];
  }
}

function save(entries: Entry[]): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
  } catch {
    // storage full or disabled — silently ignore
  }
}

export function isRecentDuplicate(
  recipients: TransferRecipientInput[],
  currency: string,
): boolean {
  const fp = fingerprint(recipients, currency);
  return load().some((e) => e.fingerprint === fp);
}

export function recordTransfer(
  recipients: TransferRecipientInput[],
  currency: string,
): void {
  const fp = fingerprint(recipients, currency);
  const existing = load().filter((e) => e.fingerprint !== fp);
  save([...existing, { fingerprint: fp, timestamp: Date.now() }]);
}
