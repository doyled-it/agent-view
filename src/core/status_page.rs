//! Polls https://status.claude.com/api/v2/summary.json for Anthropic platform status.

use crate::types::{StatusIncident, StatusIndicator, StatusPageData};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type SharedStatusData = Arc<Mutex<Option<StatusPageData>>>;

const POLL_INTERVAL: Duration = Duration::from_secs(60);
const SUMMARY_URL: &str = "https://status.claude.com/api/v2/summary.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub fn parse_response(json: &serde_json::Value) -> Option<StatusPageData> {
    let status = json.get("status")?;
    let indicator = StatusIndicator::from_str(
        status
            .get("indicator")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    let description = status
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let incidents = json
        .get("incidents")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|inc| {
                    Some(StatusIncident {
                        name: inc.get("name")?.as_str()?.to_string(),
                        status: inc
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        impact: inc
                            .get("impact")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(StatusPageData {
        indicator,
        description,
        incidents,
        last_updated: chrono::Utc::now().timestamp_millis(),
    })
}

/// Spawn the status-page monitor background thread.
pub fn spawn_monitor() -> (SharedStatusData, std::thread::JoinHandle<()>) {
    let shared: SharedStatusData = Arc::new(Mutex::new(None));
    let shared_clone = Arc::clone(&shared);

    let handle = std::thread::spawn(move || {
        monitor_loop(shared_clone);
    });

    (shared, handle)
}

fn monitor_loop(shared: SharedStatusData) {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(REQUEST_TIMEOUT)
        .timeout_read(REQUEST_TIMEOUT)
        .build();

    loop {
        if let Ok(resp) = agent.get(SUMMARY_URL).call() {
            if let Ok(json) = resp.into_json::<serde_json::Value>() {
                if let Some(data) = parse_response(&json) {
                    if let Ok(mut guard) = shared.lock() {
                        *guard = Some(data);
                    }
                }
            }
        }
        // Transient failures: keep last-known data; no state mutation.
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPERATIONAL: &str = r#"{
        "status": { "indicator": "none", "description": "All Systems Operational" },
        "incidents": []
    }"#;

    const WITH_INCIDENT: &str = r#"{
        "status": { "indicator": "major", "description": "Partial Outage" },
        "incidents": [
            {
                "name": "Elevated API error rates",
                "status": "investigating",
                "impact": "major"
            },
            {
                "name": "Console latency",
                "status": "monitoring",
                "impact": "minor"
            }
        ]
    }"#;

    #[test]
    fn test_parse_operational() {
        let v: serde_json::Value = serde_json::from_str(OPERATIONAL).unwrap();
        let data = parse_response(&v).unwrap();
        assert_eq!(data.indicator, StatusIndicator::None);
        assert_eq!(data.description, "All Systems Operational");
        assert!(data.incidents.is_empty());
    }

    #[test]
    fn test_parse_with_incidents() {
        let v: serde_json::Value = serde_json::from_str(WITH_INCIDENT).unwrap();
        let data = parse_response(&v).unwrap();
        assert_eq!(data.indicator, StatusIndicator::Major);
        assert_eq!(data.incidents.len(), 2);
        assert_eq!(data.incidents[0].name, "Elevated API error rates");
        assert_eq!(data.incidents[0].status, "investigating");
        assert_eq!(data.incidents[1].impact, "minor");
    }

    #[test]
    fn test_parse_missing_status_returns_none() {
        let v: serde_json::Value = serde_json::from_str(r#"{ "incidents": [] }"#).unwrap();
        assert!(parse_response(&v).is_none());
    }

    #[test]
    fn test_parse_unknown_indicator_defaults_to_none() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{ "status": { "indicator": "weird", "description": "?" } }"#)
                .unwrap();
        let data = parse_response(&v).unwrap();
        assert_eq!(data.indicator, StatusIndicator::None);
    }
}
