mod common;

use futures::future::join_all;
use serial_test::serial;
use uuid::Uuid;

use common::{balance, client, login, spawn_app};

#[tokio::test]
#[serial]
async fn duplicate_request_id_does_not_double_charge() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let request_id = Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "recipients": [
            { "recipient_username": "bob", "amount": "25.00" }
        ],
        "currency": "USD",
    });

    let first = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", &request_id)
        .json(&body)
        .send()
        .await
        .unwrap();
    let first_status = first.status();
    let first_text = first.text().await.unwrap();
    assert_eq!(first_status, 200, "first transfer failed: {first_text}");
    let first_body: serde_json::Value = serde_json::from_str(&first_text).unwrap();

    let alice_after_first = balance(&app.pool, "alice").await;
    let bob_after_first = balance(&app.pool, "bob").await;

    let second = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", &request_id)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 200);
    let second_body: serde_json::Value = second.json().await.unwrap();

    let alice_after_second = balance(&app.pool, "alice").await;
    let bob_after_second = balance(&app.pool, "bob").await;

    assert_eq!(
        alice_after_first, alice_after_second,
        "alice was charged twice: {alice_after_first} -> {alice_after_second}"
    );
    assert_eq!(
        bob_after_first, bob_after_second,
        "bob was credited twice: {bob_after_first} -> {bob_after_second}"
    );

    assert_eq!(
        first_body["result"]["operation_id"], second_body["result"]["operation_id"],
        "duplicate request did not return the same operation id"
    );

    let op_count: i64 = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM operations WHERE kind = 'transfer'"#,
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(op_count, 1, "ledger has more than one transfer operation");
}

#[tokio::test]
#[serial]
async fn concurrent_duplicate_requests_reject_in_flight() {
    let app = spawn_app().await;
    let c = client();
    login(&c, &app.base_url, "alice").await;

    let alice_before = balance(&app.pool, "alice").await;
    let bob_before = balance(&app.pool, "bob").await;

    let request_id = Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "recipients": [
            { "recipient_username": "bob", "amount": "25.00" }
        ],
        "currency": "USD",
    });

    let attempts = 8;
    let mut handles = Vec::with_capacity(attempts);
    for _ in 0..attempts {
        let c = c.clone();
        let url = app.base_url.clone();
        let request_id = request_id.clone();
        let body = body.clone();
        handles.push(tokio::spawn(async move {
            let resp = c
                .post(format!("{url}/transfers"))
                .header("x-request-id", &request_id)
                .json(&body)
                .send()
                .await
                .unwrap();
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap();
            (status, text)
        }));
    }
    let responses: Vec<(u16, String)> =
        join_all(handles).await.into_iter().map(|r| r.unwrap()).collect();

    // Every response must be either a completed/replayed 200 or an in-flight 409.
    // The split depends on scheduling — the first request that grabs the slot
    // wins, any duplicate that arrives while it's still executing gets 409, and
    // duplicates that arrive after it completes get the verbatim replay.
    let success_count = responses.iter().filter(|(s, _)| *s == 200).count();
    let conflict_count = responses.iter().filter(|(s, _)| *s == 409).count();
    assert!(
        success_count >= 1,
        "no request succeeded across {attempts} concurrent duplicates: {responses:?}"
    );
    assert_eq!(
        success_count + conflict_count,
        attempts,
        "concurrent duplicates returned unexpected statuses: {responses:?}"
    );

    let success_op_ids: Vec<serde_json::Value> = responses
        .iter()
        .filter(|(s, _)| *s == 200)
        .map(|(_, t)| serde_json::from_str::<serde_json::Value>(t).unwrap()["result"]["operation_id"].clone())
        .collect();
    let first_op = &success_op_ids[0];
    for op in &success_op_ids[1..] {
        assert_eq!(op, first_op, "successful duplicates returned different operation ids");
    }

    let alice_after = balance(&app.pool, "alice").await;
    let bob_after = balance(&app.pool, "bob").await;
    assert_eq!(
        alice_before - alice_after,
        2_500,
        "alice was charged more than once across {attempts} concurrent duplicates",
    );
    assert_eq!(
        bob_after - bob_before,
        2_500,
        "bob was credited more than once across {attempts} concurrent duplicates",
    );

    let op_count: i64 = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM operations WHERE kind = 'transfer'"#,
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        op_count, 1,
        "ledger recorded more than one transfer for concurrent duplicates"
    );

    // Once the in-flight request has completed, a further retry with the same
    // request_id must replay the cached response — not 409.
    let replay = c
        .post(format!("{}/transfers", app.base_url))
        .header("x-request-id", &request_id)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        replay.status().as_u16(),
        200,
        "post-completion retry did not replay cached response"
    );
}
