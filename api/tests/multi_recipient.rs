mod common;

use std::collections::HashSet;
use std::time::Duration;

use futures::future::join_all;
use serde_json::Value;
use serial_test::serial;
use tokio::time::timeout;
use uuid::Uuid;

use common::{balance, client, login, spawn_app, sum_all_balances};

// ─── Happy path ──────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn multi_recipient_transfer_records_one_operation_and_per_recipient_actions() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let alice_before = balance(&app.pool, "alice").await;
    let bob_before = balance(&app.pool, "bob").await;
    let carol_before = balance(&app.pool, "carol").await;

    let resp = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [
                { "recipient_username": "bob",   "amount": "10.00" },
                { "recipient_username": "carol", "amount": "5.00"  },
            ],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let result = &body["result"];

    assert_eq!(result["sender_username"], "alice");
    assert_eq!(result["currency"], "USD");
    assert_eq!(result["sender_balance_after"], "985.00");
    let transfers = result["transfers"].as_array().expect("transfers array");
    assert_eq!(transfers.len(), 2);
    assert_eq!(transfers[0]["recipient_username"], "bob");
    assert_eq!(transfers[0]["amount"], "10.00");
    assert_eq!(transfers[1]["recipient_username"], "carol");
    assert_eq!(transfers[1]["amount"], "5.00");
    let ids: HashSet<i64> = transfers
        .iter()
        .map(|t| t["action_id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids.len(), 2, "action ids must be unique per leg");

    let alice_after = balance(&app.pool, "alice").await;
    let bob_after = balance(&app.pool, "bob").await;
    let carol_after = balance(&app.pool, "carol").await;
    assert_eq!(alice_before - alice_after, 1_500);
    assert_eq!(bob_after - bob_before, 1_000);
    assert_eq!(carol_after - carol_before, 500);

    let op_id = result["operation_id"].as_str().unwrap();
    let op_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM operations WHERE id = $1::uuid AND kind = 'transfer'",
    )
    .bind(op_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(op_count, 1, "exactly one operation row for a multi-recipient transfer");

    let action_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM actions WHERE operation_id = $1::uuid",
    )
    .bind(op_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(action_count, 3, "one debit + two credits");

    let action_sum: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(amount), 0)::BIGINT FROM actions WHERE operation_id = $1::uuid",
    )
    .bind(op_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(action_sum, 0, "double-entry invariant");

    assert_eq!(sum_all_balances(&app.pool).await, 0, "conservation across the ledger");
}

// ─── SSE: one event per recipient ────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn multi_recipient_transfer_emits_one_sse_event_per_recipient() {
    let app = spawn_app().await;

    // Bob is the observer (any authenticated session can read the global feed).
    let bob = client();
    login(&bob, &app.base_url, "bob").await;

    let mut stream_resp = bob
        .get(format!("{}/feed/stream", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(stream_resp.status(), 200);

    // Give the broadcaster a beat to register the subscriber before we publish.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let alice = client();
    login(&alice, &app.base_url, "alice").await;
    let resp = alice
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [
                { "recipient_username": "bob",   "amount": "7.00" },
                { "recipient_username": "carol", "amount": "3.00" },
                { "recipient_username": "dave",  "amount": "1.00" },
            ],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let mut buffer = String::new();
    let mut events: Vec<Value> = Vec::new();
    let deadline = Duration::from_secs(3);
    let read = timeout(deadline, async {
        while events.len() < 3 {
            let chunk = stream_resp.chunk().await.unwrap();
            let Some(bytes) = chunk else { break };
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(blank) = buffer.find("\n\n") {
                let block = buffer[..blank].to_string();
                buffer = buffer[blank + 2..].to_string();
                let mut is_transfer = false;
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(rest) = line.strip_prefix("event:") {
                        is_transfer = rest.trim() == "transfer";
                    } else if let Some(rest) = line.strip_prefix("data:") {
                        data.push_str(rest.trim());
                    }
                }
                if is_transfer && !data.is_empty() {
                    events.push(serde_json::from_str(&data).unwrap());
                }
            }
        }
    })
    .await;
    assert!(read.is_ok(), "timed out waiting for 3 SSE events, got {}", events.len());
    assert_eq!(events.len(), 3, "one SSE event per recipient");

    let recipients: Vec<&str> = events
        .iter()
        .map(|e| e["recipient_username"].as_str().unwrap())
        .collect();
    assert_eq!(recipients, vec!["bob", "carol", "dave"]);

    for ev in &events {
        assert_eq!(ev["sender_username"], "alice");
        assert_eq!(ev["currency"], "USD");
        assert!(ev["id"].is_number(), "event id (action_id) must be present");
    }
    let op_ids: HashSet<&str> = events.iter().map(|e| e["operation_id"].as_str().unwrap()).collect();
    assert_eq!(op_ids.len(), 1, "all 3 events belong to the same operation");

    let action_ids: HashSet<i64> = events.iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert_eq!(action_ids.len(), 3, "event ids are unique per leg");
}

// ─── Atomicity ───────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn insufficient_total_funds_rolls_back_all_legs() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let alice_before = balance(&app.pool, "alice").await;
    let bob_before = balance(&app.pool, "bob").await;
    let carol_before = balance(&app.pool, "carol").await;
    assert_eq!(alice_before, 100_000);

    // Alice has $1000. Together $600 + $600 = $1200 > balance.
    let resp = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [
                { "recipient_username": "bob",   "amount": "600.00" },
                { "recipient_username": "carol", "amount": "600.00" },
            ],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422, "insufficient total funds must be rejected");

    assert_eq!(balance(&app.pool, "alice").await, alice_before, "alice unchanged");
    assert_eq!(balance(&app.pool, "bob").await, bob_before, "bob unchanged");
    assert_eq!(balance(&app.pool, "carol").await, carol_before, "carol unchanged");

    let op_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM operations WHERE kind = 'transfer'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(op_count, 0, "no partial operation written");
}

// ─── Validation ──────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn rejects_sender_in_recipient_list() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let resp = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [
                { "recipient_username": "bob",   "amount": "1.00" },
                { "recipient_username": "alice", "amount": "1.00" },
            ],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
#[serial]
async fn rejects_duplicate_recipient() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let resp = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [
                { "recipient_username": "bob", "amount": "1.00" },
                { "recipient_username": "bob", "amount": "2.00" },
            ],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
#[serial]
async fn rejects_empty_recipient_list() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let resp = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "validator rejects empty list");
}

#[tokio::test]
#[serial]
async fn rejects_unknown_recipient() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let resp = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({
            "recipients": [
                { "recipient_username": "bob",     "amount": "1.00" },
                { "recipient_username": "nobody",  "amount": "1.00" },
            ],
            "currency": "USD",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    assert_eq!(balance(&app.pool, "bob").await, 100_000, "bob not credited on partial");
}

// ─── Idempotency ─────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn idempotent_replay_returns_same_multi_recipient_body() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let req_id = Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "recipients": [
            { "recipient_username": "bob",   "amount": "12.00" },
            { "recipient_username": "carol", "amount": "8.00"  },
        ],
        "currency": "USD",
    });

    let first: Value = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", &req_id)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", &req_id)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(first["result"], second["result"], "replay must be byte-identical");

    let op_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM operations WHERE kind = 'transfer'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(op_count, 1, "no double-execute");
}

// ─── Concurrency ─────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn concurrent_multi_recipient_transfers_preserve_invariants() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let attempts = 30;
    let per_recipient_minor: i64 = 500; // $5 each, $15 per op
    let amount_str = "5.00";

    let alice_before = balance(&app.pool, "alice").await;

    let mut handles = Vec::with_capacity(attempts);
    for _ in 0..attempts {
        let cc = c.clone();
        let url = app.base_url.clone();
        handles.push(tokio::spawn(async move {
            cc.post(format!("{url}/transfers"))
                .header("x-request-id", Uuid::new_v4().to_string())
                .json(&serde_json::json!({
                    "recipients": [
                        { "recipient_username": "bob",   "amount": amount_str },
                        { "recipient_username": "carol", "amount": amount_str },
                        { "recipient_username": "dave",  "amount": amount_str },
                    ],
                    "currency": "USD",
                }))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    let statuses: Vec<u16> = join_all(handles).await.into_iter().map(|r| r.unwrap()).collect();
    let successes = statuses.iter().filter(|s| **s == 200).count() as i64;
    assert!(successes > 0, "at least some transfers should succeed");

    let alice_after = balance(&app.pool, "alice").await;
    let bob_after = balance(&app.pool, "bob").await;
    let carol_after = balance(&app.pool, "carol").await;
    let dave_after = balance(&app.pool, "dave").await;

    assert!(alice_after >= 0, "alice went negative");
    assert_eq!(
        alice_before - alice_after,
        successes * per_recipient_minor * 3,
        "alice debit matches successful ops × per-op total",
    );
    assert_eq!(bob_after - 100_000, successes * per_recipient_minor);
    assert_eq!(carol_after - 100_000, successes * per_recipient_minor);
    assert_eq!(dave_after - 100_000, successes * per_recipient_minor);

    assert_eq!(sum_all_balances(&app.pool).await, 0, "conservation");

    let actions_sum: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(amount), 0)::BIGINT FROM actions",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(actions_sum, 0, "double-entry across the whole ledger");

    // Each successful op produced exactly 4 actions (1 debit + 3 credits).
    let multi_action_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM actions WHERE operation_id IN \
         (SELECT id FROM operations WHERE kind = 'transfer')",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(multi_action_count, successes * 4);
}
