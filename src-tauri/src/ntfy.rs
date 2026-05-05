const NTFY_BASE: &str = "https://ntfy.sh";

#[derive(serde::Deserialize, Debug)]
pub struct NtfyMessage {
    pub id: String,
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
    let client = reqwest::Client::new();
    let resp = client
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

/// Poll the ntfy topic for messages newer than `since_id`.
/// Pass `None` on first call; subsequent calls pass the last seen message ID.
pub async fn poll_since(
    topic: &str,
    since_id: Option<&str>,
) -> Result<Vec<NtfyMessage>, String> {
    let url = match since_id {
        Some(id) => format!("{NTFY_BASE}/{topic}/json?poll=1&since={id}"),
        None => format!("{NTFY_BASE}/{topic}/json?poll=1&since=all"),
    };

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("ntfy poll failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("ntfy returned HTTP {}", resp.status()));
    }

    let body = resp.text().await.map_err(|e| format!("ntfy read failed: {e}"))?;
    Ok(parse_lines(&body))
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
    fn empty_body_returns_empty_vec() {
        assert_eq!(parse_lines("").len(), 0);
    }
}
