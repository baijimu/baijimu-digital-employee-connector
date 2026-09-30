//! Connector-owned HTTP response projection for the Bridge durable handoff.
//! Wire protocol is owned by Bridge; Relay implementation is not a client dependency.
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HostEventReceipt {
    #[serde(rename = "contractVersion")]
    _contract_version: HandoffContractVersion,
    event_id: String,
    app_id: String,
    #[serde(rename = "status")]
    _status: HostAcceptance,
}

#[derive(Debug, Deserialize)]
enum HandoffContractVersion {
    #[serde(rename = "2.0.0")]
    Current,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostAcceptance {
    Queued,
    NoSubscribers,
}

impl HostEventReceipt {
    pub(super) fn accepts(&self, app_id: &str, event_id: &str) -> bool {
        self.app_id == app_id && self.event_id == event_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn accepts_both_host_outcomes_from_protocol_fixture() {
        let fixtures: Vec<Value> =
            serde_json::from_str(include_str!("../../test/fixtures/host-event-receipts.json"))
                .unwrap();
        for fixture in fixtures {
            let receipt: HostEventReceipt = serde_json::from_value(fixture).unwrap();
            assert!(receipt.accepts("app", "stable"));
            assert!(!receipt.accepts("other", "stable"));
            assert!(!receipt.accepts("app", "other"));
        }
    }

    #[test]
    fn incomplete_unsupported_or_malformed_receipts_do_not_release_event() {
        let valid =
            json!({"contractVersion":"2.0.0","appId":"app","eventId":"stable","status":"queued"});
        for field in ["contractVersion", "appId", "eventId", "status"] {
            let mut invalid = valid.clone();
            invalid.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<HostEventReceipt>(invalid).is_err());
        }
        for version in ["1.0.0", "3.0.0", "2.0", "v2.0.0", "02.0.0", "2.0.0-rc.1"] {
            let mut invalid = valid.clone();
            invalid["contractVersion"] = json!(version);
            assert!(serde_json::from_value::<HostEventReceipt>(invalid).is_err());
        }
        for status in ["accepted", "rejected", "Queued"] {
            let mut invalid = valid.clone();
            invalid["status"] = json!(status);
            assert!(serde_json::from_value::<HostEventReceipt>(invalid).is_err());
        }
    }
}
