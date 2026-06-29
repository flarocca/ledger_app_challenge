mod common;

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
        "recipient_username": "bob",
        "amount": "25.00",
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
