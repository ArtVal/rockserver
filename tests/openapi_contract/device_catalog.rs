//! OpenAPI device catalog and stream playback contract verification.

use axum::{body::Body, http::Request, http::StatusCode};
use serde_json::{Value as JsonValue, json};
use serde_yaml::Value;
use tower::ServiceExt;

use super::common::{OPENAPI, assert_component_invalid, assert_component_valid, load_fixture};

#[test]
fn device_catalog_contract_is_bounded_and_stream_free() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");

    for path in [
        "/api/v1/device-control/catalog/stations",
        "/api/v1/device-control/catalog/search",
    ] {
        let operation = document["paths"]
            .get(path)
            .and_then(|path_item| path_item.get("get"))
            .unwrap_or_else(|| panic!("{path} must define GET"));
        assert_eq!(
            operation
                .get("x-rockserver-status")
                .and_then(JsonValue::as_str),
            Some("implemented"),
            "{path} must be implemented since RS-2"
        );
        let security = operation
            .get("security")
            .and_then(JsonValue::as_array)
            .expect("device catalog operations must declare security");
        assert_eq!(security.len(), 1, "{path} must have one auth alternative");
        assert!(
            security[0]
                .as_object()
                .expect("security alternative must be a mapping")
                .contains_key("RockserverBearer"),
            "{path} must accept only the native-session bearer"
        );
    }

    let browse_parameters = document["paths"]
        .get("/api/v1/device-control/catalog/stations")
        .and_then(|item| item.pointer("/get/parameters"))
        .and_then(JsonValue::as_array)
        .expect("browse must declare parameters");
    let browse_limit = browse_parameters
        .iter()
        .find(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("limit"))
        .expect("browse must bound its page size");
    assert_eq!(
        browse_limit
            .pointer("/schema/maximum")
            .and_then(JsonValue::as_i64),
        Some(20),
        "browse pages must be at most 20 stations"
    );
    let cursor = browse_parameters
        .iter()
        .find(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("cursor"))
        .expect("browse must declare its cursor");
    assert_eq!(
        cursor
            .pointer("/schema/maxLength")
            .and_then(JsonValue::as_i64),
        Some(512)
    );

    let search_parameters = document["paths"]
        .get("/api/v1/device-control/catalog/search")
        .and_then(|item| item.pointer("/get/parameters"))
        .and_then(JsonValue::as_array)
        .expect("search must declare parameters");
    assert!(
        !search_parameters
            .iter()
            .any(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("cursor")),
        "ranked search must not be cursorable"
    );
    let query = search_parameters
        .iter()
        .find(|parameter| parameter.get("name").and_then(JsonValue::as_str) == Some("q"))
        .expect("search must declare q");
    assert_eq!(
        query.get("required").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        query
            .pointer("/schema/maxLength")
            .and_then(JsonValue::as_i64),
        Some(128),
        "search queries must be at most 128 characters"
    );

    let station_properties = document
        .pointer("/components/schemas/DeviceStationDto/properties")
        .and_then(|properties| properties.as_object())
        .expect("DeviceStationDto must declare properties");
    assert_eq!(
        document
            .pointer("/components/schemas/DeviceStationDto/additionalProperties")
            .and_then(JsonValue::as_bool),
        Some(false),
        "DeviceStationDto must stay closed"
    );
    for forbidden in ["stream_url", "score", "reason", "provider_id"] {
        assert!(
            !station_properties.contains_key(forbidden),
            "DeviceStationDto must never expose {forbidden}"
        );
    }
    for page in ["DeviceCatalogPage", "DeviceSearchPage"] {
        assert_eq!(
            document
                .pointer(&format!(
                    "/components/schemas/{page}/properties/stations/maxItems"
                ))
                .and_then(JsonValue::as_i64),
            Some(20),
            "{page} must cap pages at 20 stations"
        );
    }
    assert!(
        !document
            .pointer("/components/schemas/DeviceSearchPage/properties")
            .and_then(JsonValue::as_object)
            .expect("DeviceSearchPage must declare properties")
            .contains_key("next_cursor"),
        "ranked search pages must not carry a cursor"
    );

    let station = json!({
        "id": "station-rock-001",
        "name": "Highway Rock",
        "homepage_url": null,
        "tags": ["rock"],
        "language": "en",
        "country_code": "GB",
        "codec": "AAC",
        "bitrate_kbps": 128,
        "health": "healthy"
    });
    assert_component_valid(&document, "DeviceStationDto", &station);
    let mut leaked = station.clone();
    leaked["stream_url"] = json!("https://streams.example.com/highway-rock.aac");
    assert_component_invalid(
        &document,
        "DeviceStationDto",
        &leaked,
        "station metadata must never carry a stream URL",
    );

    let mut oversized_page = json!({
        "request_id": "req_01TOOBIG",
        "stations": [],
        "next_cursor": null
    });
    let oversized_stations: Vec<_> = (0..21).map(|_| station.clone()).collect();
    oversized_page["stations"] = json!(oversized_stations);
    assert_component_invalid(
        &document,
        "DeviceCatalogPage",
        &oversized_page,
        "device catalog pages must be at most 20 stations",
    );
    assert_component_invalid(
        &document,
        "DeviceCatalogPage",
        &json!({
            "request_id": "req_01BADCURSOR",
            "stations": [],
            "next_cursor": "not a cursor $%"
        }),
        "cursors must be bounded ASCII stable station IDs",
    );

    let catalog_fixture = load_fixture("device-catalog-response.json");
    assert!(
        catalog_fixture["stations"]
            .as_array()
            .expect("catalog fixture must list stations")
            .iter()
            .all(|station| station.get("stream_url").is_none()),
        "the canonical device catalog fixture must not carry stream URLs"
    );
}

#[test]
fn station_play_stream_and_volume_bounds_are_enforceable() {
    let document: JsonValue = serde_yaml::from_str(OPENAPI)
        .map(|value: Value| serde_json::to_value(value).expect("OpenAPI must convert to JSON"))
        .expect("OpenAPI YAML must parse");

    assert_component_valid(
        &document,
        "StationCommand",
        &json!({"name": "station.play_station", "station_id": "station.jazz_fixture"}),
    );
    assert_component_valid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "station": {"name": "Fixture Jazz", "icon_url": null},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
    );
    assert_component_valid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "station": {"name": "Fixture Jazz", "icon_url": "https://icons.example.com/jazz.png"},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
    );
    assert_component_valid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "direct_stream",
            "stream_uri": "https://streams.example.com/highway-rock.aac"
        }),
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
        "the server-resolved variant must include the target presentation",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "station": {"name": "Fixture Jazz"},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
        "the target presentation must explicitly carry nullable icon_url",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station": {"name": "Fixture Jazz", "icon_url": null},
            "stream_uri": "https://streams.example.com/quiet-jazz.mp3"
        }),
        "the server-resolved variant must echo the resolved station_id",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "direct_stream",
            "station_id": "station.jazz_fixture",
            "stream_uri": "https://streams.example.com/highway-rock.aac"
        }),
        "the direct_stream variant must not claim a catalog station",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "stream_uri": "ftp://streams.example.com/quiet-jazz.mp3"
        }),
        "stream URIs must be absolute HTTP(S)",
    );
    assert_component_invalid(
        &document,
        "StationCommand",
        &json!({
            "name": "station.play_stream",
            "source": "rockserver_catalog",
            "station_id": "station.jazz_fixture",
            "stream_uri": format!("https://streams.example.com/{}.mp3", "a".repeat(2048))
        }),
        "stream URIs must be at most 2048 characters",
    );

    assert_eq!(
        document
            .pointer("/components/schemas/StationStreamUri/x-rockserver-status")
            .and_then(JsonValue::as_str),
        Some("implemented"),
        "the server-resolved play_stream dispatch is implemented since RS-3"
    );
    assert!(
        document
            .pointer("/components/schemas/VoiceStreamCancel/x-rockserver-status")
            .is_none(),
        "the voice cancel frame is implemented by RS-4"
    );

    let honest_volume = json!({
        "name": "media.volume",
        "version": 1,
        "minimum": 0,
        "maximum": 100,
        "step": 1,
        "mute": true
    });
    assert_component_valid(&document, "VolumeCapability", &honest_volume);
    for mutation in [
        json!({"minimum": -1, "maximum": 100, "step": 1}),
        json!({"minimum": 0, "maximum": 101, "step": 1}),
        json!({"minimum": 0, "maximum": 100, "step": 0}),
        json!({"minimum": 0, "maximum": 100, "step": 101}),
    ] {
        let mut broken = honest_volume.clone();
        for (field, value) in mutation.as_object().expect("mutation is an object") {
            broken[field] = value.clone();
        }
        assert_component_invalid(
            &document,
            "VolumeCapability",
            &broken,
            "Rockmobile aborts the whole snapshot on out-of-bounds volume",
        );
    }
    assert_component_valid(
        &document,
        "VolumeCommand",
        &json!({"name": "volume.set_volume", "level": 100}),
    );
    for level in [-1, 101] {
        assert_component_invalid(
            &document,
            "VolumeCommand",
            &json!({"name": "volume.set_volume", "level": level}),
            "volume levels must stay within 0..=100",
        );
    }

    let display_views = document
        .pointer("/components/schemas/DisplayCapability/properties/views/items/enum")
        .and_then(JsonValue::as_array)
        .expect("display views must stay a closed enum");
    assert_eq!(
        display_views,
        &vec![json!("text"), json!("now_playing"), json!("sensor_grid")],
        "no remote presentation types may be added beyond the frozen three"
    );
    assert_component_invalid(
        &document,
        "DisplayCapability",
        &json!({
            "name": "display.presentation",
            "version": 1,
            "views": ["station_list", "search", "loading", "offline", "playback_error"],
            "max_items": 8,
            "max_text_length": 128
        }),
        "local UI states must stay schema-invalid as presentation views",
    );
}

#[tokio::test]
async fn device_catalog_paths_are_registered_and_require_a_native_device_session() {
    struct EmptyResolver;
    #[async_trait::async_trait]
    impl rockserver::auth::NativeSessionResolver for EmptyResolver {
        async fn resolve_active_native_session(
            &self,
            _access_hash: &rockserver::auth::SecretHash,
        ) -> Result<
            Option<rockserver::auth::ActiveSession>,
            rockserver::auth::NativeSessionLookupError,
        > {
            Ok(None)
        }
    }

    let app = rockserver::http::router_with_search_service_and_native_session_resolver(
        rockserver::search::SearchService::new(std::sync::Arc::new(
            rockserver::search::InMemoryStationRepository::with_builtin_catalog().unwrap(),
        )),
        std::time::Duration::from_secs(5),
        std::sync::Arc::new(EmptyResolver),
    );
    for path in [
        "/api/v1/device-control/catalog/stations",
        "/api/v1/device-control/catalog/search?q=jazz",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(
                        "authorization",
                        format!("Bearer {}", rockserver::http::TEST_API_BEARER_TOKEN),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{path} is registered since RS-2 and must require a valid device session"
        );
    }
}
