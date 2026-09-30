//! In-memory integration coverage for the public station-search route.

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const API_TOKEN: &str = rockserver::http::TEST_API_BEARER_TOKEN;

#[tokio::test]
async fn successful_search_returns_normalized_query_and_station_results() {
    let (status, body) = search(json!({"query": "rock"})).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(body["normalized_query"]["locale"], "en-US");
    assert_eq!(body["normalized_query"]["tags"], json!(["rock"]));
    let station = &body["stations"][0];
    assert!(station["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(
        station["stream_url"]
            .as_str()
            .is_some_and(|url| url.starts_with("http"))
    );
    assert_eq!(station["health"], "unknown");
    assert!(station.get("favicon_url").is_some());
    assert!(station["favicon_url"].is_null());
}

#[tokio::test]
async fn unmatched_query_returns_an_empty_station_list() {
    let (status, body) = search(json!({"query": "baroque opera"})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stations"], json!([]));
}

#[tokio::test]
async fn limit_is_applied_after_ranking() {
    let (status, body) = search(json!({"query": "rock", "limit": 1})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stations"].as_array().unwrap().len(), 1);
    let (_, full) = search(json!({"query": "rock"})).await;
    assert_eq!(body["stations"][0]["id"], full["stations"][0]["id"]);
}

#[tokio::test]
async fn pagination_offset_and_total_matches() {
    let (status, first_page) = search(json!({"query": "rock", "limit": 1, "offset": 0})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first_page["stations"].as_array().unwrap().len(), 1);
    let total = first_page["total"].as_u64().unwrap();
    assert!(total > 1);
    assert_eq!(first_page["has_more"], true);

    let (status, second_page) = search(json!({"query": "rock", "limit": 1, "offset": 1})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second_page["stations"].as_array().unwrap().len(), 1);
    assert_eq!(second_page["total"], total);
    assert_ne!(first_page["stations"][0]["id"], second_page["stations"][0]["id"]);

    let (_, end_page) = search(json!({"query": "rock", "limit": 10, "offset": total})).await;
    assert_eq!(end_page["stations"].as_array().unwrap().len(), 0);
    assert_eq!(end_page["has_more"], false);
}

#[tokio::test]
async fn equal_score_results_use_station_id_as_a_stable_tie_break() {
    let (_, first) = search(json!({"query": "rock"})).await;
    let (_, second) = search(json!({"query": "rock"})).await;
    let first_ids = station_ids(&first);
    let second_ids = station_ids(&second);

    assert!(!first_ids.is_empty());
    assert_eq!(first_ids, second_ids);
}

#[tokio::test]
async fn excluded_station_ids_do_not_appear_in_results() {
    let (status, body) = search(json!({
        "query": "rock",
        "exclude_station_ids": ["181-fm-power"]
    }))
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(!station_ids(&body).is_empty());
    assert!(!station_ids(&body).contains(&"181-fm-power"));
}

#[tokio::test]
async fn malformed_json_returns_a_contract_compliant_400_error() {
    let response = rockserver::http::router()
        .oneshot(
            Request::post("/api/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {API_TOKEN}"))
                .body(Body::from(r#"{"query":"jazz""#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response_body(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_contract_error(
        &body,
        "malformed_request",
        "Request body must contain valid JSON.",
    );
}

#[tokio::test]
async fn semantic_validation_returns_a_contract_compliant_422_error() {
    let (status, body) = search(json!({"query": "   ", "limit": 0})).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_contract_error(&body, "validation_failed", "Request validation failed.");
    assert!(body["details"].get("query").is_some());
    assert!(body["details"].get("limit").is_some());
}

async fn search(payload: Value) -> (StatusCode, Value) {
    let response = rockserver::http::router()
        .oneshot(
            Request::post("/api/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {API_TOKEN}"))
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (status, response_body(response).await)
}

#[tokio::test]
async fn api_v1_search_is_anonymous() {
    let app = rockserver::http::router();
    let public_search = app
        .clone()
        .oneshot(
            Request::post("/api/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"query":"jazz"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(public_search.status(), StatusCode::OK);

    let health = app
        .oneshot(Request::get("/health/live").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
}

#[tokio::test]
async fn anonymous_search_rejects_the_burst_before_work_is_started() {
    let app = rockserver::http::router();
    for _ in 0..10 {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"query":"jazz"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let rejected = app
        .oneshot(
            Request::post("/api/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"query":"jazz"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rejected.headers()[header::RETRY_AFTER], "60");
    assert_contract_error(
        &response_body(rejected).await,
        "rate_limited",
        "Request rate limit exceeded.",
    );
}

async fn response_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn station_ids(body: &Value) -> Vec<&str> {
    body["stations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|station| station["id"].as_str().unwrap())
        .collect()
}

fn assert_contract_error(body: &Value, code: &str, message: &str) {
    assert_eq!(body["code"], code);
    assert_eq!(body["message"], message);
    assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(body["details"].is_object());
}
