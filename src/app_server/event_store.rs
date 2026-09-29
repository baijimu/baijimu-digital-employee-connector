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

#[derive(Clone, Debug)]
struct ConnectorEvent {
    sequence: u64,
    received_at: String,
    method: String,
    params: Value,
}

#[derive(Default)]
struct EventState {
    sequence: u64,
    retained: VecDeque<ConnectorEvent>,
}

pub(super) struct EventStore {
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
    pub(super) fn push(&self, method: &str, params: Value) {
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
        drop(state);
        if let Some(publisher) = &self.publisher {
            publisher.publish_raw(event);
            if let Some(domain_event) = domain_event {
                publisher.publish_domain(domain_event);
            }
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
            .map(|event| {
                json!({
                    "sequence": event.sequence,
                    "receivedAt": event.received_at,
                    "method": event.method,
                    "params": event.params,
                })
            })
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

    fn publish_raw(&self, event: ConnectorEvent) {
        self.enqueue(PublishJob {
            event_name: "codexNotification",
            event_id: random_event_id(),
            occurred_at: event.received_at.clone(),
            payload: json!({
                "sequence": event.sequence,
                "receivedAt": event.received_at,
                "method": event.method,
                "params": event.params,
            }),
        });
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
                    .json::<relay::contracts::device_events::LocalEventAccepted>()
                    .is_ok_and(|receipt| {
                        receipt.event_id == job.event_id && receipt.app_id == self.app_id
                    }),
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
    pub(super) fn in_memory() -> Self {
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
