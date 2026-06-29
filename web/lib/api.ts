export const API_URL =
  process.env.NEXT_PUBLIC_API_URL || "http://localhost:4000";

function uuidv4(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return (crypto as { randomUUID(): string }).randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

type ApiInit = Omit<RequestInit, "body"> & {
  body?: BodyInit | Record<string, unknown> | null;
  idempotencyKey?: string;
};

export async function api<T>(path: string, init: ApiInit = {}): Promise<T> {
  const { body, headers, idempotencyKey, ...rest } = init;
  const requestId = idempotencyKey || uuidv4();
  const finalHeaders: Record<string, string> = {
    "X-Request-Id": requestId,
    ...(body && typeof body === "object" && !(body instanceof FormData)
      ? { "Content-Type": "application/json" }
      : {}),
    ...(headers as Record<string, string> | undefined),
  };
  const finalBody =
    body && typeof body === "object" && !(body instanceof FormData)
      ? JSON.stringify(body)
      : (body as BodyInit | null | undefined);

  const res = await fetch(`${API_URL}${path}`, {
    ...rest,
    body: finalBody,
    headers: finalHeaders,
    credentials: "include",
  });

  const text = await res.text();
  const data = text ? JSON.parse(text) : {};

  if (!res.ok) {
    const message = data?.error?.message || res.statusText;
    const code = data?.error?.code || "ERROR";
    throw new ApiError(code, message, res.status);
  }

  return data.result as T;
}

export class ApiError extends Error {
  constructor(
    public code: string,
    public message: string,
    public status: number,
  ) {
    super(message);
  }
}

export type MeResponse = {
  user_id: number;
  username: string;
  email: string;
  balance: string;
  currency: string;
};

export type FeedItem = {
  sender_username: string;
  recipient_username: string;
  amount: string;
  currency: string;
  created_at: string;
};

export type TransferResult = {
  operation_id: string;
  sender_username: string;
  recipient_username: string;
  amount: string;
  currency: string;
  sender_balance_after: string;
  created_at: string;
};

export function newRequestId(): string {
  return uuidv4();
}

const CURRENCY_SYMBOLS: Record<string, string> = { USD: "$" };

export function formatMoney(amount: string, currency: string): string {
  const symbol = CURRENCY_SYMBOLS[currency] ?? "";
  return `${symbol}${amount} ${currency}`.trim();
}
