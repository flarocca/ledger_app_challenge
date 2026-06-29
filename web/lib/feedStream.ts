import { API_URL, newRequestId } from "./api";

export type FeedEvent = {
  operation_id: string;
  sender_username: string;
  recipient_username: string;
  amount: string;
  currency: string;
  created_at: string;
};

export function openFeedStream(
  onEvent: (e: FeedEvent) => void,
  onError?: (err: unknown) => void,
): () => void {
  const controller = new AbortController();
  const run = async () => {
    try {
      const res = await fetch(`${API_URL}/feed/stream`, {
        method: "GET",
        credentials: "include",
        headers: { "X-Request-Id": newRequestId() },
        signal: controller.signal,
      });
      if (!res.body) return;
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      let currentEvent: { event?: string; data?: string } = {};

      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let nl: number;
        while ((nl = buffer.indexOf("\n")) >= 0) {
          const line = buffer.slice(0, nl).replace(/\r$/, "");
          buffer = buffer.slice(nl + 1);
          if (line === "") {
            if (currentEvent.event === "transfer" && currentEvent.data) {
              try {
                onEvent(JSON.parse(currentEvent.data) as FeedEvent);
              } catch {
                // ignore parse errors
              }
            }
            currentEvent = {};
          } else if (line.startsWith("event:")) {
            currentEvent.event = line.slice(6).trim();
          } else if (line.startsWith("data:")) {
            currentEvent.data = (currentEvent.data || "") + line.slice(5).trim();
          }
        }
      }
    } catch (err) {
      if (!controller.signal.aborted) onError?.(err);
    }
  };
  void run();
  return () => controller.abort();
}
