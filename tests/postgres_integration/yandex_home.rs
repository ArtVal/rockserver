//! Integration tests for Yandex Home connection lifecycle and encrypted token storage.

use std::env;

use rockserver::auth::SecretHash;
use rockserver::persistence::{EncryptedYandexHomeToken, PostgresAccountStore};
use uuid::Uuid;

/// Exercises migration 0024, Yandex Home OAuth state lifecycle, and encrypted token persistence.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_yandex_home_connection_lifecycle_and_active_owner() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let user_id = Uuid::new_v4();
    store
        .create_user_with_display_name(user_id, "yandex test account")
        .await
        .unwrap();

    assert!(!store.has_yandex_home_connection(user_id).await.unwrap());
    assert!(
        store
            .yandex_home_connection(user_id)
            .await
            .unwrap()
            .is_none()
    );

    let state_hash = SecretHash::new([42u8; 32]);
    assert!(
        store
            .create_yandex_home_oauth_state(user_id, &state_hash)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .consume_yandex_home_oauth_state(&state_hash)
            .await
            .unwrap(),
        Some(user_id)
    );
    assert_eq!(
        store
            .consume_yandex_home_oauth_state(&state_hash)
            .await
            .unwrap(),
        None
    );

    let token = EncryptedYandexHomeToken {
        ciphertext: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
        nonce: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
    };
    assert!(
        store
            .save_yandex_home_connection(user_id, &token)
            .await
            .unwrap()
    );
    assert!(store.has_yandex_home_connection(user_id).await.unwrap());
    assert_eq!(
        store.yandex_home_connection(user_id).await.unwrap(),
        Some(token)
    );

    assert!(store.revoke_yandex_home_connection(user_id).await.unwrap());
    assert!(!store.has_yandex_home_connection(user_id).await.unwrap());
    assert!(
        store
            .yandex_home_connection(user_id)
            .await
            .unwrap()
            .is_none()
    );
    store.close().await;
}
