use super::*;
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::oneshot;

const NOW: &str = "/api/v1/stations/station/now-playing";
const EVENTS: &str = "/api/v1/stations/station/events";

async fn json(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap()
}

async fn next_event(body: &mut Body) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let data = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(data.starts_with("event: snapshot\n"), "{data}");
    serde_json::from_str(
        data.lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap()
}

fn title_block(title: &str) -> Vec<u8> {
    let mut block = format!("StreamTitle='{title}';").into_bytes();
    block.resize(32, 0);
    let mut bytes = b"ABCD\x02".to_vec();
    bytes.extend(block);
    bytes
}

#[tokio::test]
async fn metadata_is_authorized_and_never_opens_upstream() {
    let (url, connections) = upstream("HTTP/1.1 200 OK\r\n\r\n", b"audio", false).await;
    let (router, _) = build_relay_router(url);
    for path in [NOW, EVENTS] {
        assert_eq!(
            get(router.clone(), path, false, false).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            get(
                router.clone(),
                &format!("{path}?url=http://example.org"),
                false,
                true
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            get(
                router.clone(),
                "/api/v1/stations/unknown/now-playing",
                false,
                true
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    let empty = json(get(router.clone(), NOW, false, true).await).await;
    assert_eq!(empty["state"], "missing");
    assert!(empty["rawTitle"].is_null());
    let response = get(router, EVENTS, false, true).await;
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    let mut body = response.into_body();
    assert_eq!(next_event(&mut body).await["state"], "missing");
    assert_eq!(connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn title_changes_arrive_without_restarting_audio_and_reconnect_gets_latest() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/radio", listener.local_addr().unwrap());
    let (release, wait) = oneshot::channel::<()>();
    let connections = Arc::new(AtomicUsize::new(0));
    let seen = connections.clone();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        seen.fetch_add(1, Ordering::SeqCst);
        let mut request = [0; 2048];
        let request_len = socket.read(&mut request).await.unwrap();
        assert!(request_len > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nicy-metaint: 4\r\nContent-Type: audio/mpeg\r\n\r\n")
            .await
            .unwrap();
        socket.write_all(&title_block("First")).await.unwrap();
        let _ = wait.await;
        socket.write_all(&title_block("Second")).await.unwrap();
        socket.write_all(b"ABCD\x00").await.unwrap();
    });
    let (router, _) = build_relay_router(url);
    let mut events = get(router.clone(), EVENTS, false, true).await.into_body();
    assert_eq!(next_event(&mut events).await["state"], "missing");
    let audio = get(
        router.clone(),
        "/api/v1/stations/station/stream",
        true,
        true,
    )
    .await;
    let audio_task = tokio::spawn(async move { to_bytes(audio.into_body(), 4096).await.unwrap() });
    assert_eq!(next_event(&mut events).await["rawTitle"], "First");
    assert_eq!(
        json(get(router.clone(), NOW, false, true).await).await["state"],
        "fresh"
    );
    release.send(()).unwrap();
    assert_eq!(next_event(&mut events).await["rawTitle"], "Second");
    let bytes = audio_task.await.unwrap();
    assert!(bytes.windows(6).any(|window| window == b"Second"));
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    drop(events);
    let mut reconnected = get(router, EVENTS, false, true).await.into_body();
    assert_eq!(next_event(&mut reconnected).await["rawTitle"], "Second");
    assert_eq!(connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stale_snapshot_disconnect_and_slow_subscriber_release_slots() {
    let (router, relay) = build_relay_router("http://example.org/radio".into());
    relay.titles.lock().unwrap().insert(
        "station".into(),
        TitleSnapshot {
            raw_title: "Old".into(),
            observed_at: Instant::now() - Duration::from_secs(121),
            updated_at: "2026-10-01T00:00:00Z".into(),
        },
    );
    let stale = json(get(router.clone(), NOW, false, true).await).await;
    assert_eq!(stale["state"], "stale");
    assert_eq!(stale["rawTitle"], "Old");
    let mut body = get(router.clone(), EVENTS, false, true).await.into_body();
    assert_eq!(next_event(&mut body).await["state"], "stale");
    drop(body);
    for _ in 0..40 {
        if relay.subscribers.available_permits() == 64 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(relay.subscribers.available_permits(), 64);

    let slow = get(router.clone(), EVENTS, false, true).await;
    for i in 0..10 {
        let snapshot = TitleSnapshot {
            raw_title: i.to_string(),
            observed_at: Instant::now(),
            updated_at: "2026-10-01T00:00:00Z".into(),
        };
        relay
            .titles
            .lock()
            .unwrap()
            .insert("station".into(), snapshot.clone());
        let _ = relay.changes.send(("station".into(), snapshot));
    }
    for _ in 0..40 {
        if relay.subscribers.available_permits() == 64 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(relay.subscribers.available_permits(), 64);
    drop(slow);
    let mut reconnected = get(router, EVENTS, false, true).await.into_body();
    assert_eq!(next_event(&mut reconnected).await["rawTitle"], "9");
}

#[tokio::test]
async fn subscriber_capacity_returns_retryable_error() {
    let (router, relay) = build_relay_router("http://example.org/radio".into());
    let permits = Arc::clone(&relay.subscribers)
        .try_acquire_many_owned(64)
        .unwrap();
    let response = get(router, EVENTS, false, true).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "5");
    drop(permits);
}
