mod common;

use futures::future::join_all;
use serial_test::serial;
use uuid::Uuid;

use common::{balance, client, login, spawn_app, sum_all_balances};

#[tokio::test]
#[serial]
async fn one_hundred_concurrent_transfers_preserve_invariants() {
    let app = spawn_app().await;

    let alice_client = client();
    login(&alice_client, &app.base_url, "alice").await;

    let alice_starting = balance(&app.pool, "alice").await;
    let bob_starting = balance(&app.pool, "bob").await;
    assert_eq!(alice_starting, 100_000);
    assert_eq!(bob_starting, 100_000);

    let total_in_system_before = sum_all_balances(&app.pool).await;
    assert_eq!(total_in_system_before, 0);

    let amount_per_transfer: i64 = 1_500;
    let amount_decimal = "15.00";
    let attempts = 100;

    let mut handles = Vec::with_capacity(attempts);
    for _ in 0..attempts {
        let c = alice_client.clone();
        let url = app.base_url.clone();
        handles.push(tokio::spawn(async move {
            c.post(format!("{url}/transfers"))
                .header("x-request-id", Uuid::new_v4().to_string())
                .json(&serde_json::json!({
                    "recipients": [
                        { "recipient_username": "bob", "amount": amount_decimal }
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

    let successes = statuses.iter().filter(|s| **s == 200).count();
    let failures = statuses.iter().filter(|s| **s != 200).count();
    assert_eq!(successes + failures, attempts);

    let alice_after = balance(&app.pool, "alice").await;
    let bob_after = balance(&app.pool, "bob").await;

    assert!(alice_after >= 0, "alice went negative: {alice_after}");

    let total_after = sum_all_balances(&app.pool).await;
    assert_eq!(total_after, 0, "money was created or destroyed");

    let expected_max_successes = (alice_starting / amount_per_transfer) as usize;
    assert!(
        successes <= expected_max_successes,
        "too many transfers succeeded: {successes} > {expected_max_successes}"
    );

    assert_eq!(
        alice_starting - alice_after,
        bob_after - bob_starting,
        "alice debit and bob credit do not match"
    );
    assert_eq!(
        bob_after - bob_starting,
        (successes as i64) * amount_per_transfer,
        "moved amount does not match success count"
    );

    let actions_sum: i64 = sqlx::query_scalar::<_, i64>(
        r#"SELECT COALESCE(SUM(amount), 0)::BIGINT FROM actions"#
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(actions_sum, 0, "double-entry violated across the ledger");
}
