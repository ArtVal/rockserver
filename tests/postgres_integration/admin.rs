//! Integration tests for administrator persistence and authentication flows.

use std::env;

use rockserver::admin::{
    AdminAuditFilter, AdminLoginAttempt, AdminLoginOutcome, AdminPasswordHash, AdminPrincipal,
    AdminPrincipalStatus, AdminRequestOutcome, AdminRequestRecord, AdminSecurityEvent,
    AdminSecurityEventType, AdminStore, AdminUsername, NewAdminPasswordCredential, NewAdminSession,
};
use rockserver::admin_bootstrap::{AdminBootstrapConfig, bootstrap_admin};
use rockserver::auth::SecretHash;
use rockserver::persistence::PostgresAdminStore;
use sqlx::PgPool;
use uuid::Uuid;

/// Covers administrator-only migrations and hashed credential/session persistence against PostgreSQL.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_admin_identity_foundation_rejects_invalid_hash_state() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAdminStore::connect(&database_url)
        .await
        .expect("admin migrations must succeed");
    // The singleton principal from migration 0019 requires a clean slate per test.
    let pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::query(
        "TRUNCATE admin_request_records, admin_security_events, admin_password_credentials, admin_sessions, admin_login_attempts, admin_principals",
    )
    .execute(&pool)
    .await
    .unwrap();
    let principal_id = Uuid::new_v4();
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Active,
        })
        .await
        .unwrap();
    store
        .create_password_credential(NewAdminPasswordCredential {
            id: Uuid::new_v4(),
            principal_id,
            password_hash: AdminPasswordHash::parse(
                "$argon2id$v=19$m=65536,t=3,p=4$salt$hash".to_owned(),
            )
            .unwrap(),
        })
        .await
        .unwrap();
    assert!(
        store
            .active_password_credential(principal_id)
            .await
            .unwrap()
            .is_some()
    );
    let mut token_hash_bytes = [0_u8; 32];
    token_hash_bytes[..16].copy_from_slice(principal_id.as_bytes());
    token_hash_bytes[16..].copy_from_slice(principal_id.as_bytes());
    let token_hash = SecretHash::new(token_hash_bytes);
    store
        .create_session(NewAdminSession {
            id: Uuid::new_v4(),
            principal_id,
            token_hash: token_hash.clone(),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    assert!(
        store
            .find_active_session(&token_hash)
            .await
            .unwrap()
            .is_some()
    );
    store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id: Some(principal_id),
            account_key_hash: SecretHash::new([51; 32]),
            source_ip_hash: SecretHash::new([52; 32]),
            outcome: AdminLoginOutcome::Succeeded,
        })
        .await
        .unwrap();
    store
        .record_security_event(AdminSecurityEvent {
            id: Uuid::new_v4(),
            principal_id: Some(principal_id),
            session_id: store
                .find_active_session(&token_hash)
                .await
                .unwrap()
                .map(|session| session.id),
            source_ip_hash: Some(SecretHash::new([52; 32])),
            event_type: AdminSecurityEventType::SessionCreated,
        })
        .await
        .unwrap();

    assert!(sqlx::query("INSERT INTO admin_password_credentials (id, principal_id, password_hash) VALUES ($1, $2, $3)")
        .bind(Uuid::new_v4()).bind(principal_id).bind("not-a-hash")
        .execute(&pool).await.is_err());
    assert!(sqlx::query("INSERT INTO admin_sessions (id, principal_id, token_hash, expires_at) VALUES ($1, $2, $3, $4::timestamptz)")
        .bind(Uuid::new_v4()).bind(principal_id).bind(vec![1_u8; 31]).bind("2035-01-01T00:00:00Z")
        .execute(&pool).await.is_err());
    pool.close().await;
}

/// Verifies protected bootstrap is atomic, one-time, and safe under concurrent PostgreSQL attempts.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_admin_bootstrap_is_atomic_and_idempotent() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAdminStore::connect(&database_url)
        .await
        .expect("admin migrations must succeed");
    let pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::query(
        "TRUNCATE admin_request_records, admin_security_events, admin_password_credentials, admin_sessions, admin_login_attempts, admin_principals",
    )
    .execute(&pool)
    .await
    .unwrap();

    let first = AdminBootstrapConfig::from_values(
        Some("bootstrap-admin".to_owned()),
        Some("a-deterministic-test-password".to_owned()),
    )
    .unwrap();
    let second = AdminBootstrapConfig::from_values(
        Some("other-bootstrap-admin".to_owned()),
        Some("a-different-test-password".to_owned()),
    )
    .unwrap();
    let (first, second) = tokio::join!(
        bootstrap_admin(&store, first),
        bootstrap_admin(&store, second)
    );
    assert!(
        matches!(first, Ok(rockserver::admin::AdminBootstrapOutcome::Created))
            ^ matches!(
                second,
                Ok(rockserver::admin::AdminBootstrapOutcome::Created)
            )
    );
    assert!(
        matches!(
            first,
            Ok(rockserver::admin::AdminBootstrapOutcome::AlreadyExists)
        ) ^ matches!(
            second,
            Ok(rockserver::admin::AdminBootstrapOutcome::AlreadyExists)
        )
    );

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_principals")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_password_credentials")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM admin_security_events WHERE event_type = 'admin_created'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    // The audit projection is covered here explicitly instead of relying on records another
    // test happened to leave behind: record one refresh through the store, then read it back.
    let principal_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM admin_principals")
        .fetch_one(&pool)
        .await
        .unwrap();
    let session_id = Uuid::new_v4();
    store
        .create_session(NewAdminSession {
            id: session_id,
            principal_id,
            token_hash: SecretHash::new([71; 32]),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    store
        .record_request(AdminRequestRecord {
            id: Uuid::new_v4(),
            request_id: "bootstrap-audit-request".to_owned(),
            principal_id,
            session_id,
            endpoint: "/api/v1/admin/auth/refresh",
            outcome: AdminRequestOutcome::Succeeded,
            duration_ms: 1,
        })
        .await
        .unwrap();
    let audit = store
        .list_audit(AdminAuditFilter {
            from: None,
            until: None,
            action: Some("/api/v1/admin/auth/refresh".to_owned()),
            outcome: Some("succeeded".to_owned()),
            offset: 0,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, "/api/v1/admin/auth/refresh");
    pool.close().await;
}

/// Exercises administrator login lookup, durable throttle count, and immediate session revocation.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn postgres_admin_auth_queries_are_durable_and_revocable() {
    let database_url = env::var("TEST_DATABASE_URL")
        .expect("set TEST_DATABASE_URL to an isolated PostgreSQL database");
    let store = PostgresAdminStore::connect(&database_url).await.unwrap();
    let pool = PgPool::connect(&database_url).await.unwrap();
    sqlx::query("TRUNCATE admin_request_records, admin_security_events, admin_password_credentials, admin_sessions, admin_login_attempts, admin_principals")
        .execute(&pool).await.unwrap();
    let config = AdminBootstrapConfig::from_values(
        Some("auth-admin".to_owned()),
        Some("a-deterministic-test-password".to_owned()),
    )
    .unwrap();
    assert!(matches!(
        bootstrap_admin(&store, config).await,
        Ok(rockserver::admin::AdminBootstrapOutcome::Created)
    ));
    let username = AdminUsername::parse("auth-admin".to_owned()).unwrap();
    let credential = store
        .login_credential(&username)
        .await
        .unwrap()
        .expect("active bootstrap credential");
    let account_hash = SecretHash::new([61; 32]);
    let source_hash = SecretHash::new([62; 32]);
    store
        .record_login_attempt(AdminLoginAttempt {
            id: Uuid::new_v4(),
            principal_id: Some(credential.credential.principal_id),
            account_key_hash: account_hash.clone(),
            source_ip_hash: source_hash.clone(),
            outcome: AdminLoginOutcome::Failed,
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .recent_failed_login_count(&account_hash, &source_hash)
            .await
            .unwrap(),
        1
    );
    let token_hash = SecretHash::new([63; 32]);
    let session_id = Uuid::new_v4();
    store
        .create_session(NewAdminSession {
            id: session_id,
            principal_id: credential.credential.principal_id,
            token_hash: token_hash.clone(),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    let replacement_hash = SecretHash::new([64; 32]);
    let replacement_id = Uuid::new_v4();
    assert!(
        store
            .rotate_session(
                session_id,
                NewAdminSession {
                    id: replacement_id,
                    principal_id: credential.credential.principal_id,
                    token_hash: replacement_hash.clone(),
                    ttl_seconds: 60,
                },
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .find_active_session(&token_hash)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .find_active_session(&replacement_hash)
            .await
            .unwrap()
            .unwrap()
            .id,
        replacement_id
    );
    store
        .record_request(AdminRequestRecord {
            id: Uuid::new_v4(),
            request_id: "admin-integration-request".to_owned(),
            principal_id: credential.credential.principal_id,
            session_id: replacement_id,
            endpoint: "/api/v1/admin/auth/refresh",
            outcome: AdminRequestOutcome::Succeeded,
            duration_ms: 1,
        })
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admin_request_records")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
}
