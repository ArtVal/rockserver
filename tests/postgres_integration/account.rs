//! Integration tests for account registration, passkey authentication, browser sessions,
//! and desktop pairing flows against PostgreSQL.

use std::env;

use rockserver::auth::{
    NewBrowserSession, NewPairingRequest, NewPairingSession, NewSession, NewWebAuthnChallenge,
    OwnedDevice, PairingCompletionOutcome, SecretHash, WebAuthnCeremony,
};
use rockserver::persistence::PostgresAccountStore;
use uuid::Uuid;

use crate::common::repository_pool;

/// Covers the approved passkey-only account persistence invariants against PostgreSQL.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_b2_browser_pairing_webauthn_and_rate_limits() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let user_id = Uuid::new_v4();
    store.create_user(user_id).await.unwrap();
    let other_user_id = Uuid::new_v4();
    store.create_user(other_user_id).await.unwrap();

    assert!(
        store
            .create_passkey_credential(
                Uuid::new_v4(),
                user_id,
                b"credential-id",
                b"public-key",
                0,
                &[],
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .advance_passkey_sign_count(b"credential-id", 1)
            .await
            .unwrap()
    );
    assert!(
        store
            .find_passkey_credential_for_user(user_id, b"credential-id")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .find_passkey_credential_for_user(other_user_id, b"credential-id")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .find_passkey_credential(other_user_id.as_bytes())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !store
            .advance_passkey_sign_count(b"credential-id", 0)
            .await
            .unwrap()
    );

    let browser_session_id = Uuid::new_v4();
    assert!(
        store
            .create_browser_session(NewBrowserSession {
                session_id: browser_session_id,
                user_id,
                session_token_hash: &SecretHash::new([11; 32]),
                csrf_hash: &SecretHash::new([1; 32]),
                passkey_reauthenticated_at_rfc3339: "2035-01-01T00:00:00Z",
                expires_at_rfc3339: "2035-02-01T00:00:00Z",
            })
            .await
            .unwrap()
    );

    let challenge_id = Uuid::new_v4();
    let challenge_hash = SecretHash::new([2; 32]);
    assert!(
        store
            .create_webauthn_challenge(NewWebAuthnChallenge {
                challenge_id,
                challenge_hash: &challenge_hash,
                state_blob: b"{}",
                ceremony: WebAuthnCeremony::Authentication,
                rp_id: "alex.vault57.ru",
                origin: "https://alex.vault57.ru",
                expires_at_rfc3339: "2035-01-01T00:00:00Z",
                user_id: Some(user_id),
                browser_session_id: Some(browser_session_id),
                pairing_request_id: None,
            })
            .await
            .unwrap()
    );
    assert!(
        store
            .consume_webauthn_challenge(
                challenge_id,
                &challenge_hash,
                WebAuthnCeremony::Authentication,
                "https://alex.vault57.ru",
                "alex.vault57.ru",
            )
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_webauthn_challenge(
                challenge_id,
                &challenge_hash,
                WebAuthnCeremony::Authentication,
                "https://alex.vault57.ru",
                "alex.vault57.ru",
            )
            .await
            .unwrap()
    );

    let request_id = Uuid::new_v4();
    assert!(
        store
            .create_pairing_request(NewPairingRequest {
                request_id,
                desktop_token_hash: &SecretHash::new([3; 32]),
                approval_secret_hash: &SecretHash::new([4; 32]),
                short_code_hash: &SecretHash::new([5; 32]),
                verification_phrase: "BLUE-MOON",
                device_display_name: "Test desktop",
                device_type: "rockcast_windows",
                app_version: Some("test"),
                expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    assert!(
        store
            .lookup_pairing_request(&SecretHash::new([5; 32]))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        !store
            .approve_pairing_request(
                request_id,
                user_id,
                browser_session_id,
                &SecretHash::new([99; 32]),
                "BLUE-MOON",
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .approve_pairing_request(
                request_id,
                user_id,
                browser_session_id,
                &SecretHash::new([4; 32]),
                "BLUE-MOON",
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .complete_pairing(
                request_id,
                &SecretHash::new([99; 32]),
                NewPairingSession {
                    session_id: Uuid::new_v4(),
                    device_id: Uuid::new_v4(),
                    access_hash: &SecretHash::new([98; 32]),
                    access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                    device_secret_hash: &SecretHash::new([97; 32]),
                },
            )
            .await
            .unwrap()
            == PairingCompletionOutcome::InvalidProof,
        "a wrong desktop proof must not consume the approved request"
    );
    let expired_request_id = Uuid::new_v4();
    assert!(
        store
            .create_pairing_request(NewPairingRequest {
                request_id: expired_request_id,
                desktop_token_hash: &SecretHash::new([12; 32]),
                approval_secret_hash: &SecretHash::new([13; 32]),
                short_code_hash: &SecretHash::new([14; 32]),
                verification_phrase: "SILVER-STAR",
                device_display_name: "Expired desktop",
                device_type: "rockcast_windows",
                app_version: None,
                expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    assert!(
        store
            .approve_pairing_request(
                expired_request_id,
                user_id,
                browser_session_id,
                &SecretHash::new([13; 32]),
                "SILVER-STAR",
            )
            .await
            .unwrap()
    );
    let expiry_pool = repository_pool(&database_url).await;
    sqlx::query(
        "UPDATE pairing_requests SET created_at = now() - interval '2 minutes', \
         expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(expired_request_id)
    .execute(&expiry_pool)
    .await
    .unwrap();
    expiry_pool.close().await;
    assert!(
        store
            .complete_pairing(
                expired_request_id,
                &SecretHash::new([12; 32]),
                NewPairingSession {
                    session_id: Uuid::new_v4(),
                    device_id: Uuid::new_v4(),
                    access_hash: &SecretHash::new([15; 32]),
                    access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                    device_secret_hash: &SecretHash::new([16; 32]),
                },
            )
            .await
            .unwrap()
            == PairingCompletionOutcome::NoLongerAvailable,
        "an expired approved request must not issue a session"
    );
    let device_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let completion = store
        .complete_pairing(
            request_id,
            &SecretHash::new([3; 32]),
            NewPairingSession {
                session_id,
                device_id,
                access_hash: &SecretHash::new([6; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                device_secret_hash: &SecretHash::new([7; 32]),
            },
        )
        .await
        .unwrap();
    let PairingCompletionOutcome::Completed(completion) = completion else {
        panic!("approved request must derive its owner");
    };
    assert_eq!(completion.user_id, user_id);
    assert_eq!(completion.device_id, device_id);
    assert!(
        store
            .complete_pairing(
                request_id,
                &SecretHash::new([3; 32]),
                NewPairingSession {
                    session_id: Uuid::new_v4(),
                    device_id: Uuid::new_v4(),
                    access_hash: &SecretHash::new([8; 32]),
                    access_expires_at_rfc3339: "2035-01-01T00:10:00Z",
                    device_secret_hash: &SecretHash::new([9; 32]),
                },
            )
            .await
            .unwrap()
            == PairingCompletionOutcome::NoLongerAvailable
    );

    let rate_key = SecretHash::new([10; 32]);
    assert!(
        store
            .consume_rate_limit(&rate_key, "2035-01-01T00:00:00Z", "2035-01-01T00:15:00Z", 1,)
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_rate_limit(&rate_key, "2035-01-01T00:00:00Z", "2035-01-01T00:15:00Z", 1,)
            .await
            .unwrap()
    );
    store.delete_account(user_id).await.unwrap();
    let inspection = repository_pool(&database_url).await;
    let browser_revoked = sqlx::query_scalar::<_, bool>(
        "SELECT revoked_at IS NOT NULL FROM browser_sessions WHERE id = $1",
    )
    .bind(browser_session_id)
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert!(
        browser_revoked,
        "account deletion must revoke browser sessions"
    );
    inspection.close().await;
    store.close().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_account_session_rotation_deletion_and_ownership() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url)
        .await
        .expect("account migrations must succeed");
    let owner = Uuid::new_v4();
    let foreign_user = Uuid::new_v4();
    store.create_user(owner).await.expect("first user inserts");
    assert!(
        store.create_user(owner).await.is_err(),
        "duplicate account is rejected"
    );
    store
        .create_user(foreign_user)
        .await
        .expect("second user inserts");
    let device = OwnedDevice {
        id: Uuid::new_v4(),
        user_id: owner,
        device_display_name: "Desktop".into(),
        device_type: "windows".into(),
        created_at: "unused".into(),
        last_seen_at: None,
    };
    assert!(
        store
            .create_device(&device)
            .await
            .expect("owner device inserts")
    );
    assert_eq!(
        store
            .find_owned_device(foreign_user, device.id)
            .await
            .unwrap(),
        None
    );
    // The store stamps created_at server-side; the caller's placeholder never persists.
    let stored = store
        .find_owned_device(owner, device.id)
        .await
        .unwrap()
        .expect("the owner sees the device");
    assert_eq!(stored.id, device.id);
    assert_eq!(stored.user_id, device.user_id);
    assert_eq!(stored.device_display_name, device.device_display_name);
    assert_eq!(stored.device_type, device.device_type);
    assert_eq!(stored.last_seen_at, None);
    assert!(
        time::OffsetDateTime::parse(
            &stored.created_at,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok(),
        "created_at must be the server-stamped instant, not the caller's placeholder"
    );

    let session = Uuid::new_v4();
    assert!(
        store
            .create_session(NewSession {
                session_id: session,
                user_id: owner,
                device_id: device.id,
                access_hash: &SecretHash::new([1; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:00:00Z",
            })
            .await
            .unwrap()
    );
    assert!(store.delete_account(owner).await.unwrap());
    assert_eq!(
        store.find_owned_device(owner, device.id).await.unwrap(),
        None
    );
    let inspection = repository_pool(&database_url).await;
    let state = sqlx::query_as::<_, (String, bool)>("SELECT u.status, EXISTS(SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.revoked_at IS NULL) FROM users u WHERE u.id = $1")
        .bind(owner).fetch_one(&inspection).await.unwrap();
    assert_eq!(state, ("deleted".to_owned(), false));
    inspection.close().await;
}

/// Verifies the browser account centre keeps management actions owner-scoped and revocation blocks access.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_browser_account_centre_owns_rename_and_revoke() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAccountStore::connect(&database_url).await.unwrap();
    let owner = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    store.create_user(owner).await.unwrap();
    store.create_user(foreign).await.unwrap();
    let device = OwnedDevice {
        id: Uuid::new_v4(),
        user_id: owner,
        device_display_name: "Living room PC".into(),
        device_type: "rockcast_windows".into(),
        created_at: "unused".into(),
        last_seen_at: None,
    };
    assert!(store.create_device(&device).await.unwrap());
    let cookie = SecretHash::new([41; 32]);
    let csrf = SecretHash::new([42; 32]);
    assert!(
        store
            .create_browser_session(NewBrowserSession {
                session_id: Uuid::new_v4(),
                user_id: owner,
                session_token_hash: &cookie,
                csrf_hash: &csrf,
                passkey_reauthenticated_at_rfc3339: "2035-01-01T00:00:00Z",
                expires_at_rfc3339: "2035-02-01T00:00:00Z"
            })
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .browser_session_user_with_csrf(&cookie, &csrf)
            .await
            .unwrap(),
        Some(owner)
    );
    let listed = store.list_browser_devices(owner).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].device_display_name, "Living room PC");
    assert_eq!(listed[0].session_status, "inactive");
    assert!(
        !store
            .rename_owned_device(foreign, device.id, "Foreign name")
            .await
            .unwrap()
    );
    assert!(
        store
            .rename_owned_device(owner, device.id, "Studio PC")
            .await
            .unwrap()
    );
    assert_eq!(
        store.list_browser_devices(owner).await.unwrap()[0].device_display_name,
        "Studio PC"
    );
    assert!(
        store
            .create_session(NewSession {
                session_id: Uuid::new_v4(),
                user_id: owner,
                device_id: device.id,
                access_hash: &SecretHash::new([44; 32]),
                access_expires_at_rfc3339: "2035-01-01T00:15:00Z",
            })
            .await
            .unwrap()
    );
    assert!(!store.revoke_owned_device(foreign, device.id).await.unwrap());
    assert!(store.revoke_owned_device(owner, device.id).await.unwrap());
    store.close().await;
}
