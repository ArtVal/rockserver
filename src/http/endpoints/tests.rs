//! Unit tests for HTTP endpoints and router construction.

use std::sync::Arc;

use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use uuid::Uuid;

use super::{
    RouterBuilder,
    health::{HealthResponse, HealthStatus},
    router,
};
use crate::{
    admin::{
        AdminPasswordHash, AdminPrincipal, AdminPrincipalStatus, AdminStore, AdminUsername,
        FakeAdminStore, NewAdminBootstrap, NewAdminSession,
    },
    auth::SecretHash,
};

#[tokio::test]
async fn liveness_returns_stable_json_response() {
    assert_health_endpoint("/health/live").await;
}

#[tokio::test]
async fn direct_server_does_not_serve_the_admin_spa() {
    let response = router()
        .oneshot(Request::get("/admin").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_icon_route_returns_a_non_cacheable_miss_without_configured_storage() {
    let response = router()
        .oneshot(
            Request::get("/api/v1/stations/station-rock-001/icon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn protected_admin_station_read_model_requires_and_accepts_a_revocable_session() {
    let token = "admin-test-token";
    let store = Arc::new(FakeAdminStore::default());
    let principal_id = Uuid::new_v4();
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Active,
        })
        .await
        .unwrap();
    store
        .create_session(NewAdminSession {
            id: Uuid::new_v4(),
            principal_id,
            token_hash: SecretHash::new(Sha256::digest(token.as_bytes()).into()),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    let app = RouterBuilder::default()
        .with_api_bearer_token("unrelated")
        .with_admin_store(store)
        .build();
    let denied = app
        .clone()
        .oneshot(
            Request::get("/api/v1/admin/stations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let origin_rejected = app
        .clone()
        .oneshot(
            Request::post("/api/v1/admin/icons/import")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(origin_rejected.status(), StatusCode::FORBIDDEN);
    let manual_icon_requires_configured_storage = app
        .clone()
        .oneshot(
            Request::put("/api/v1/admin/stations/station-rock-001/icon")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header("origin", super::transport::FIRST_PARTY_ORIGIN)
                .header("x-forwarded-proto", "https")
                .body(Body::from("not an image"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        manual_icon_requires_configured_storage.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let allowed = app
        .oneshot(
            Request::get("/api/v1/admin/stations")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(allowed.headers()[header::CACHE_CONTROL], "no-store");
    let body = allowed.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["limit"].as_u64(),
        Some(50)
    );
    assert!(
        !serde_json::from_slice::<serde_json::Value>(&body).unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn admin_refresh_atomically_replaces_the_bearer_and_records_safe_request_metadata() {
    let token = "admin-refresh-old-token";
    let store = Arc::new(FakeAdminStore::default());
    let records = Arc::clone(&store);
    let principal_id = Uuid::new_v4();
    store
        .create_principal(AdminPrincipal {
            id: principal_id,
            status: AdminPrincipalStatus::Active,
        })
        .await
        .unwrap();
    store
        .create_session(NewAdminSession {
            id: Uuid::new_v4(),
            principal_id,
            token_hash: SecretHash::new(Sha256::digest(token.as_bytes()).into()),
            ttl_seconds: 60,
        })
        .await
        .unwrap();
    let app = RouterBuilder::default()
        .with_api_bearer_token("unrelated")
        .with_admin_store(store)
        .build();
    let refresh = Request::post("/api/v1/admin/auth/refresh")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("origin", super::transport::FIRST_PARTY_ORIGIN)
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(refresh).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let fresh_token = serde_json::from_slice::<serde_json::Value>(
        &response.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let old = app
        .clone()
        .oneshot(
            Request::get("/api/v1/admin/session")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
    let current = app
        .oneshot(
            Request::get("/api/v1/admin/session")
                .header(header::AUTHORIZATION, format!("Bearer {fresh_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(current.status(), StatusCode::OK);
    let request_records = records.request_records();
    assert_eq!(request_records.len(), 2);
    assert!(
        request_records
            .iter()
            .all(|record| !record.request_id.contains(token))
    );
    assert!(request_records.iter().all(|record| matches!(
        record.endpoint,
        "/api/v1/admin/auth/refresh" | "/api/v1/admin/session"
    )));
}

#[tokio::test]
async fn admin_login_accepts_valid_credentials_and_rejects_invalid_credentials() {
    let store = Arc::new(FakeAdminStore::default());
    let principal_id = Uuid::new_v4();
    let hash = Argon2::default()
        .hash_password(b"correct password", &SaltString::generate(&mut OsRng))
        .unwrap()
        .to_string();
    store
        .bootstrap_admin(NewAdminBootstrap {
            principal_id,
            credential_id: Uuid::new_v4(),
            security_event_id: Uuid::new_v4(),
            username: AdminUsername::parse("admin".to_owned()).unwrap(),
            password_hash: AdminPasswordHash::parse(hash).unwrap(),
        })
        .await
        .unwrap();
    let app = RouterBuilder::default()
        .with_api_bearer_token("unrelated")
        .with_admin_store(store)
        .with_local_admin_origin(Some("http://127.0.0.1:3000".to_owned()))
        .build();
    let login = |password: &str| {
        Request::post("/api/v1/admin/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .header("origin", "http://127.0.0.1:3000")
            .body(Body::from(
                serde_json::json!({"username":"admin", "password":password}).to_string(),
            ))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(login("incorrect"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = app.oneshot(login("correct password")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(
        serde_json::from_slice::<serde_json::Value>(
            &response.into_body().collect().await.unwrap().to_bytes()
        )
        .unwrap()["access_token"]
            .as_str()
            .unwrap()
            .len()
            >= 32
    );
}

#[tokio::test]
async fn readiness_returns_stable_json_response() {
    assert_health_endpoint("/health/ready").await;
}

#[tokio::test]
async fn pairing_creation_fails_closed_without_the_postgres_account_store() {
    let response = router()
        .oneshot(
            Request::post("/api/v1/pairing-requests")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"device_display_name":"Test desktop","device_type":"windows"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
        "auth_unavailable"
    );
}

#[tokio::test]
async fn pairing_completion_rejects_a_client_supplied_owner() {
    let response = router()
        .oneshot(
            Request::post("/api/v1/pairing-requests/00000000-0000-0000-0000-000000000000/complete")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"desktop_token":"0123456789abcdef","user_id":"00000000-0000-0000-0000-000000000000"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn passkey_authentication_rejects_a_client_supplied_account_identifier() {
    let response = router()
        .oneshot(
            Request::post("/api/v1/auth/passkeys/authentication/verify")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"challenge_id":"00000000-0000-0000-0000-000000000000","user_id":"00000000-0000-0000-0000-000000000000","id":"x","authenticatorData":"x","signature":"x","clientDataJSON":"x","userHandle":"x"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn browser_session_refresh_rejects_direct_requests() {
    let response = router()
        .oneshot(
            Request::post("/api/v1/auth/browser-session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn browser_sync_rejects_direct_requests() {
    let response = router()
        .oneshot(
            Request::post("/api/v1/browser/sync")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

async fn assert_health_endpoint(uri: &str) {
    let response = router()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: HealthResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        payload,
        HealthResponse {
            status: HealthStatus::Ok
        }
    );
    assert_eq!(body.as_ref(), br#"{"status":"ok"}"#);
}
