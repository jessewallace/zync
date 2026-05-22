const NTFY_BASE: &str = "https://ntfy.sh";

static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct NtfyMessage {
    pub id: String,
    pub time: u64,
    pub event: String,
    pub message: String,
}

/// Parse newline-delimited JSON from ntfy, returning only `event == "message"` entries.
pub fn parse_lines(body: &str) -> Vec<NtfyMessage> {
    body.lines()
        .filter_map(|line| serde_json::from_str::<NtfyMessage>(line).ok())
        .filter(|m| m.event == "message")
        .collect()
}

/// Publish a file ID to the ntfy topic. Fire-and-forget.
pub async fn publish(topic: &str, file_id: &str) -> Result<(), String> {
    let resp = client()
        .post(format!("{NTFY_BASE}/{topic}"))
        .header("Content-Type", "text/plain")
        .body(file_id.to_string())
        .send()
        .await
        .map_err(|e| format!("ntfy publish failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("ntfy returned HTTP {}", resp.status()));
    }
    Ok(())
}

/// Poll the ntfy topic for messages newer than `since`.
/// `since` is either a message ID or a Unix timestamp string.
/// The caller is responsible for advancing `since` between calls
/// so messages published between polls are never skipped.
pub async fn poll_since(
    topic: &str,
    since: &str,
) -> Result<Vec<NtfyMessage>, String> {
    let url = format!("{NTFY_BASE}/{topic}/json?poll=1&since={since}");

    let resp = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("ntfy poll failed: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(vec![]);
    }
    if !status.is_success() {
        return Err(format!("ntfy returned HTTP {}", status));
    }

    let body = resp.text().await.map_err(|e| format!("ntfy read failed: {e}"))?;
    Ok(parse_lines(&body))
}

/// Parse a version number from an ntfy message body.
/// New format is a plain decimal integer (e.g. "7").
pub fn parse_version_message(msg: &str) -> Option<u32> {
    msg.trim().parse::<u32>().ok()
}

/// Publish a version number to the ntfy topic.
pub async fn publish_version(topic: &str, version: u32) -> Result<(), String> {
    publish(topic, &version.to_string()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_event() {
        let line = r#"{"id":"abc123","time":1234567890,"event":"message","topic":"test","message":"FILEID1"}"#;
        let msgs = parse_lines(line);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, "abc123");
        assert_eq!(msgs[0].time, 1234567890);
        assert_eq!(msgs[0].message, "FILEID1");
    }

    #[test]
    fn skip_open_event() {
        let lines = "{\"id\":\"x\",\"time\":1,\"event\":\"open\",\"topic\":\"t\",\"message\":\"\"}\n\
                     {\"id\":\"y\",\"time\":2,\"event\":\"message\",\"topic\":\"t\",\"message\":\"ABC\"}";
        let msgs = parse_lines(lines);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].message, "ABC");
    }

    #[test]
    fn max_by_time_picks_newest() {
        let lines = "{\"id\":\"old\",\"time\":100,\"event\":\"message\",\"topic\":\"t\",\"message\":\"OLDER\"}\n\
                     {\"id\":\"new\",\"time\":200,\"event\":\"message\",\"topic\":\"t\",\"message\":\"NEWER\"}";
        let msgs = parse_lines(lines);
        let latest = msgs.iter().max_by_key(|m| m.time).unwrap();
        assert_eq!(latest.id, "new");
        assert_eq!(latest.message, "NEWER");
    }

    #[test]
    fn empty_body_returns_empty_vec() {
        assert_eq!(parse_lines("").len(), 0);
    }

    #[test]
    fn parse_version_message_valid() {
        assert_eq!(super::parse_version_message("7"), Some(7));
        assert_eq!(super::parse_version_message("  7\n"), Some(7));
    }

    #[test]
    fn parse_version_message_invalid() {
        assert_eq!(super::parse_version_message("ABC123"), None);
        assert_eq!(super::parse_version_message(""), None);
    }
}
