use crate::{
    connector_home, events, random_event_id, thread_state, timestamp,
    DOMAIN_EVENT_RETRY_BASE_DELAY, MAX_EVENTS,
};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Mutex;
use std::thread;

const EVENT_PUBLISH_QUEUE_CAPACITY: usize = 256;
/// Per-subscriber live buffer. A subscriber that falls this far behind is
/// dropped and resumes with its last cursor (or receives a resync).
const STREAM_SUBSCRIBER_CAPACITY: usize = 1024;

#[derive(Clone, Debug)]
pub(crate) struct ConnectorEvent {
    sequence: u64,
    received_at: String,
    method: String,
    params: Value,
}

#[derive(Default)]
struct EventState {
    sequence: u64,
    retained: VecDeque<ConnectorEvent>,
    subscribers: Vec<SyncSender<ConnectorEvent>>,
}

/// A position in this process's notification stream: `{streamId}:{sequence}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StreamCursor {
    pub(crate) stream_id: String,
    pub(crate) sequence: u64,
}

impl StreamCursor {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let (stream_id, sequence) = value.rsplit_once(':')?;
        if stream_id.is_empty() || stream_id.chars().any(char::is_control) {
            return None;
        }
        Some(Self {
            stream_id: stream_id.to_string(),
            sequence: sequence.parse().ok()?,
        })
    }

    pub(crate) fn encode(&self) -> String {
        format!("{}:{}", self.stream_id, self.sequence)
    }
}

pub(crate) struct StreamSubscription {
    /// Set when the requested cursor cannot be continued; entries follow it.
    pub(crate) resync: Option<StreamCursor>,
    pub(crate) backlog: Vec<StreamEntry>,
    pub(crate) live: Receiver<ConnectorEvent>,
    pub(crate) stream_id: String,
}

pub(crate) struct StreamEntry {
    pub(crate) cursor: StreamCursor,
    pub(crate) data: Value,
}

impl StreamSubscription {
    pub(crate) fn entry(&self, event: ConnectorEvent) -> StreamEntry {
        event.into_entry(&self.stream_id)
    }
}

impl ConnectorEvent {
    fn payload(&self) -> Value {
        json!({
            "sequence": self.sequence,
            "receivedAt": self.received_at,
            "method": self.method,
            "params": self.params,
        })
    }

    fn into_entry(self, stream_id: &str) -> StreamEntry {
        StreamEntry {
            cursor: StreamCursor {
                stream_id: stream_id.to_string(),
                sequence: self.sequence,
            },
            data: self.payload(),
        }
    }
}

pub(crate) struct EventStore {
    state: Mutex<EventState>,
    publisher: Option<EventPublisher>,
    stream_id: String,
}
struct EventPublisher {
    sender: SyncSender<PublishJob>,
}

struct PublishJob {
    event_name: &'static str,
    event_id: String,
    occurred_at: String,
    payload: Value,
}

struct PublisherWorker {
    app_id: String,
    endpoint: String,
    token: String,
    client: reqwest::blocking::Client,
}
impl EventStore {
    pub(crate) fn push(&self, method: &str, params: Value) {
        if method == "turn/completed" {
            if let Some(thread_id) = params.get("threadId").and_then(Value::as_str) {
                if let Err(error) = thread_state::mark_thread_unread(&connector_home(), thread_id) {
                    eprintln!("failed to persist unread Codex thread {thread_id}: {error}");
                }
            }
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sequence += 1;
        let sequence = state.sequence;
        let received_at = timestamp();
        let domain_event = events::normalize_codex_notification(
            method,
            &params,
            &received_at,
            &self.stream_id,
            sequence,
        )
        .map_err(|error| {
            eprintln!("failed to normalize Codex domain event {method}: {error}");
            error
        })
        .ok()
        .flatten();
        let event = ConnectorEvent {
            sequence,
            received_at,
            method: method.to_string(),
            params,
        };
        state.retained.push_back(event.clone());
        while state.retained.len() > MAX_EVENTS {
            state.retained.pop_front();
        }
        state
            .subscribers
            .retain(|subscriber| subscriber.try_send(event.clone()).is_ok());
        drop(state);
        if let (Some(publisher), Some(domain_event)) = (&self.publisher, domain_event) {
            publisher.publish_domain(domain_event);
        }
    }

    /// Opens the live stream after `after`. Retained entries newer than the
    /// cursor are replayed first; a cursor from another process run or one
    /// that left the retention window yields a resync at the current tail.
    pub(crate) fn subscribe(&self, after: Option<&StreamCursor>) -> StreamSubscription {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let (sender, live) = mpsc::sync_channel(STREAM_SUBSCRIBER_CAPACITY);
        let current = StreamCursor {
            stream_id: self.stream_id.clone(),
            sequence: state.sequence,
        };
        let oldest = state
            .retained
            .front()
            .map_or(state.sequence, |event| event.sequence.saturating_sub(1));
        let (resync, backlog) = match after {
            None => (None, Vec::new()),
            Some(after)
                if after.stream_id == self.stream_id
                    && after.sequence >= oldest
                    && after.sequence <= state.sequence =>
            {
                let backlog = state
                    .retained
                    .iter()
                    .filter(|event| event.sequence > after.sequence)
                    .cloned()
                    .map(|event| event.into_entry(&self.stream_id))
                    .collect();
                (None, backlog)
            }
            Some(_) => (Some(current), Vec::new()),
        };
        state.subscribers.push(sender);
        StreamSubscription {
            resync,
            backlog,
            live,
            stream_id: self.stream_id.clone(),
        }
    }

    pub(super) fn recent(&self, body: &Value) -> Value {
        let after_sequence = body
            .get("afterSequence")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let limit = body
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(100)
            .clamp(1, 500) as usize;
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let events = state
            .retained
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(ConnectorEvent::payload)
            .collect::<Vec<_>>();
        json!({"latestSequence": state.sequence, "events": events})
    }

    pub(super) fn summary(&self) -> (u64, usize) {
        self.state
            .lock()
            .map(|state| (state.sequence, state.retained.len()))
            .unwrap_or((0, 0))
    }
}

impl EventPublisher {
    fn from_env() -> Option<Self> {
        let app_id = env::var("BAIJIMU_LOCAL_APP_ID").ok()?;
        let endpoint = env::var("BAIJIMU_LOCAL_APP_EVENT_ENDPOINT").ok()?;
        let token_path = env::var("BAIJIMU_LOCAL_APP_EVENT_TOKEN_FILE").ok()?;
        let token = fs::read_to_string(token_path).ok()?.trim().to_string();
        if endpoint.trim().is_empty() || token.is_empty() {
            return None;
        }
        let (sender, receiver) = mpsc::sync_channel(EVENT_PUBLISH_QUEUE_CAPACITY);
        let worker = PublisherWorker {
            app_id,
            endpoint,
            token,
            client: reqwest::blocking::Client::new(),
        };
        thread::spawn(move || worker.run(receiver));
        Some(Self { sender })
    }

    fn publish_domain(&self, event: events::DomainEvent) {
        self.enqueue(PublishJob {
            event_name: event.name,
            event_id: event.event_id,
            occurred_at: event.occurred_at,
            payload: event.payload,
        });
    }

    fn enqueue(&self, job: PublishJob) {
        // Apply bounded backpressure until the host owns the event. Never discard on full.
        self.sender.send(job).expect("event handoff worker stopped");
    }
}

impl PublisherWorker {
    fn run(self, receiver: Receiver<PublishJob>) {
        while let Ok(job) = receiver.recv() {
            self.publish(job);
        }
    }

    fn publish(&self, job: PublishJob) {
        let request = json!({
            "appId": self.app_id,
            "event": job.event_name,
            "eventId": job.event_id,
            "payload": job.payload,
            "occurredAt": job.occurred_at,
        });
        let mut attempt = 0_u32;
        loop {
            let result = self
                .client
                .post(&self.endpoint)
                .bearer_auth(&self.token)
                .timeout(std::time::Duration::from_secs(10))
                .json(&request)
                .send();
            let accepted = match result {
                Ok(response) if response.status().is_success() => response
                    .json::<super::host_receipt::HostEventReceipt>()
                    .is_ok_and(|receipt| receipt.accepts(&self.app_id, &job.event_id)),
                Ok(_) | Err(_) => false,
            };
            if accepted {
                return;
            }
            if attempt == 0 || attempt % 10 == 0 {
                eprintln!(
                    "event {} ({}) is awaiting durable Bridge acceptance; retained for retry",
                    job.event_name, job.event_id
                );
            }
            let multiplier = 1_u32 << attempt.min(8);
            thread::sleep(DOMAIN_EVENT_RETRY_BASE_DELAY * multiplier);
            attempt = attempt.saturating_add(1);
        }
    }
}

impl EventStore {
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self {
            state: Mutex::new(EventState::default()),
            publisher: None,
            stream_id: "test-stream".into(),
        }
    }

    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(EventState::default()),
            publisher: EventPublisher::from_env(),
            stream_id: random_event_id(),
        }
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn retries_same_event_until_matching_typed_durable_acceptance() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/events", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in [
                (503, json!({"error":"capacity"})),
                (202, json!({"accepted":true})),
                (
                    202,
                    json!({"contractVersion":"2.0.0","eventId":"other","appId":"app","status":"queued"}),
                ),
                (
                    202,
                    json!({"contractVersion":"2.0.0","eventId":"stable","appId":"app","status":"queued"}),
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0_u8; 1024];
                let body = loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(start) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..start]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= start + 4 + length {
                            break bytes[start + 4..start + 4 + length].to_vec();
                        }
                    }
                };
                requests.push(serde_json::from_slice::<Value>(&body).unwrap());
                let body = response.1.to_string();
                write!(stream,"HTTP/1.1 {} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.0,body.len(),body).unwrap();
            }
            requests
        });
        PublisherWorker {
            app_id: "app".into(),
            endpoint,
            token: "test".into(),
            client: reqwest::blocking::Client::new(),
        }
        .publish(PublishJob {
            event_name: "codexNotification",
            event_id: "stable".into(),
            occurred_at: "time".into(),
            payload: json!({"text":"event"}),
        });
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests.iter().all(|request| request == &requests[0]));
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    fn cursor(stream_id: &str, sequence: u64) -> StreamCursor {
        StreamCursor {
            stream_id: stream_id.into(),
            sequence,
        }
    }

    fn sequences(subscription: &StreamSubscription) -> Vec<u64> {
        subscription
            .backlog
            .iter()
            .map(|entry| entry.cursor.sequence)
            .collect()
    }

    #[test]
    fn cursor_round_trips_and_rejects_malformed_values() {
        assert_eq!(StreamCursor::parse("abc:12"), Some(cursor("abc", 12)));
        assert_eq!(StreamCursor::parse("a:b:3"), Some(cursor("a:b", 3)));
        assert_eq!(cursor("abc", 12).encode(), "abc:12");
        for invalid in ["", "abc", ":3", "abc:", "abc:-1", "abc:x", "a\nb:1"] {
            assert_eq!(StreamCursor::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn subscription_replays_retained_entries_or_resyncs() {
        let store = EventStore::in_memory();
        for index in 0..3 {
            store.push("item/started", json!({"index": index}));
        }
        let live = store.subscribe(None);
        assert!(live.resync.is_none() && live.backlog.is_empty());

        let resumed = store.subscribe(Some(&cursor("test-stream", 1)));
        assert!(resumed.resync.is_none());
        assert_eq!(sequences(&resumed), vec![2, 3]);
        assert_eq!(
            resumed.backlog[0].data,
            json!({"sequence":2,"receivedAt":resumed.backlog[0].data["receivedAt"],"method":"item/started","params":{"index":1}})
        );
        assert!(store
            .subscribe(Some(&cursor("test-stream", 0)))
            .resync
            .is_none());
        assert!(store
            .subscribe(Some(&cursor("test-stream", 3)))
            .backlog
            .is_empty());

        for stale in [cursor("previous-run", 2), cursor("test-stream", 9)] {
            let subscription = store.subscribe(Some(&stale));
            assert_eq!(subscription.resync, Some(cursor("test-stream", 3)));
            assert!(subscription.backlog.is_empty());
        }

        store.push("item/completed", json!({}));
        assert_eq!(live.live.try_recv().unwrap().sequence, 4);
        assert_eq!(resumed.live.try_recv().unwrap().sequence, 4);
    }

    #[test]
    fn cursor_outside_retention_resyncs() {
        let store = EventStore::in_memory();
        for _ in 0..(MAX_EVENTS + 5) {
            store.push("item/delta", json!({}));
        }
        let oldest_kept = 6;
        assert!(store
            .subscribe(Some(&cursor("test-stream", oldest_kept - 1)))
            .resync
            .is_none());
        let subscription = store.subscribe(Some(&cursor("test-stream", oldest_kept - 2)));
        assert_eq!(
            subscription.resync,
            Some(cursor("test-stream", (MAX_EVENTS + 5) as u64))
        );
    }

    #[test]
    fn slow_or_closed_subscribers_are_dropped_without_blocking_producers() {
        let store = EventStore::in_memory();
        let slow = store.subscribe(None);
        drop(store.subscribe(None));
        for _ in 0..(STREAM_SUBSCRIBER_CAPACITY + 1) {
            store.push("item/delta", json!({}));
        }
        assert_eq!(store.state.lock().unwrap().subscribers.len(), 0);
        let mut received = 0;
        while slow.live.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, STREAM_SUBSCRIBER_CAPACITY);
        assert!(matches!(
            slow.live.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }
}
