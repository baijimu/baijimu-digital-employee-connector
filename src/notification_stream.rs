//! `GET /streams/notifications`: live app-server notifications as SSE.
//! Each entry's id is `{streamId}:{sequence}`; Bridge resumes with
//! `Last-Event-ID` and receives `resync` when this process cannot continue it.
use crate::app_server::event_store::{StreamCursor, StreamEntry, StreamSubscription};
use crate::HttpError;
use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

pub(crate) const NOTIFICATION_STREAM_PATH: &str = "/streams/notifications";
const KEEPALIVE: Duration = Duration::from_secs(15);

pub(crate) fn parse_cursor(last_event_id: Option<&str>) -> Result<Option<StreamCursor>, HttpError> {
    last_event_id
        .map(|value| {
            StreamCursor::parse(value).ok_or_else(|| {
                HttpError::coded(
                    400,
                    "Last-Event-ID 不是有效的流游标",
                    "INVALID_STREAM_CURSOR",
                    json!({}),
                )
            })
        })
        .transpose()
}

/// Writes until the client disconnects or falls too far behind.
pub(crate) fn serve(
    stream: &mut TcpStream,
    subscription: StreamSubscription,
) -> Result<(), String> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        )
        .map_err(|error| error.to_string())?;
    if let Some(cursor) = &subscription.resync {
        let data = json!({"streamId": cursor.stream_id, "latestSequence": cursor.sequence});
        write_event(stream, "resync", &cursor.encode(), &data)?;
    }
    // The backlog and the live channel are taken under one store lock, so
    // they neither overlap nor leave a gap.
    for entry in &subscription.backlog {
        write_entry(stream, entry)?;
    }
    loop {
        match subscription.live.recv_timeout(KEEPALIVE) {
            Ok(event) => write_entry(stream, &subscription.entry(event))?,
            Err(RecvTimeoutError::Timeout) => stream
                .write_all(b": keepalive\n\n")
                .and_then(|_| stream.flush())
                .map_err(|error| error.to_string())?,
            // The store dropped this subscriber because it fell behind; the
            // client reconnects with its last cursor.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn write_entry(stream: &mut TcpStream, entry: &StreamEntry) -> Result<(), String> {
    write_event(stream, "notification", &entry.cursor.encode(), &entry.data)
}

fn write_event(stream: &mut TcpStream, event: &str, id: &str, data: &Value) -> Result<(), String> {
    // serde_json never emits raw newlines, so one `data:` line is exact.
    let frame = format!("event: {event}\nid: {id}\ndata: {data}\n\n");
    stream
        .write_all(frame.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_server::event_store::EventStore;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    #[test]
    fn manifest_declares_this_stream_path_and_capability() {
        let manifest: Value = serde_json::from_str(include_str!("../connector.json")).unwrap();
        assert_eq!(
            manifest["notificationStream"]["path"],
            NOTIFICATION_STREAM_PATH
        );
        assert!(manifest["hostRequirements"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("connector.notification-stream.v1")));
        assert!(manifest["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["name"] != "codexNotification"));
    }

    #[test]
    fn invalid_last_event_id_is_rejected_with_a_stable_code() {
        let error = parse_cursor(Some("nope")).unwrap_err();
        assert_eq!(error.status, 400);
        assert_eq!(error.code, Some(json!("INVALID_STREAM_CURSOR")));
        assert_eq!(parse_cursor(None).unwrap(), None);
    }

    #[test]
    fn serves_resync_then_live_entries_as_server_sent_events() {
        let store = EventStore::in_memory();
        store.push("item/started", json!({"n": 1}));
        let subscription = store.subscribe(StreamCursor::parse("previous-run:7").as_ref());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let _ = serve(&mut socket, subscription);
        });
        let client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(client);
        let mut head = String::new();
        while !head.ends_with("\r\n\r\n") {
            reader.read_line(&mut head).unwrap();
        }
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("Content-Type: text/event-stream\r\n"));

        let mut read_event = || {
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\n" {
                    return lines;
                }
                lines.push(line.trim_end().to_string());
            }
        };
        assert_eq!(
            read_event(),
            vec![
                "event: resync".to_string(),
                "id: test-stream:1".into(),
                r#"data: {"latestSequence":1,"streamId":"test-stream"}"#.into(),
            ]
        );
        store.push("item/completed", json!({"n": 2}));
        let event = read_event();
        assert_eq!(event[0], "event: notification");
        assert_eq!(event[1], "id: test-stream:2");
        let data: Value = serde_json::from_str(event[2].strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(data["sequence"], 2);
        assert_eq!(data["method"], "item/completed");
        assert_eq!(data["params"], json!({"n": 2}));
        drop(reader);
        drop(server);
    }
}
