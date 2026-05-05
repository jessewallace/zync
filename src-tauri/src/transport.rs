// Litterbox (litterbox.catbox.moe) transport adapter.
// Swap transport: replace `upload` and `download` only.
//
// Sync code format: ZEN-{key_hex}-{file_id}
//   key_hex  — 6 uppercase hex chars (3 random bytes), the encryption key
//   file_id  — alphanumeric file identifier extracted from the Litterbox URL
//
// This lets the pull machine reconstruct BOTH the download URL and the
// decryption key from a single code, with no relay server required.

const LITTERBOX_API: &str =
    "https://litterbox.catbox.moe/resources/internals/api.php";

pub struct UploadResult {
    pub url: String,
}

/// Upload encrypted bytes to Litterbox with a 1-hour expiry.
/// Returns the plain-text URL on success.
pub async fn upload(data: Vec<u8>) -> Result<UploadResult, String> {
    let file_part = reqwest::multipart::Part::bytes(data)
        .file_name("zync.bin")
        .mime_str("application/octet-stream")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("reqtype", "fileupload")
        .text("time", "1h")
        .part("fileToUpload", file_part);

    let client = reqwest::Client::new();
    let response = client
        .post(LITTERBOX_API)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Litterbox returned HTTP {}", response.status()));
    }

    let url = response
        .text()
        .await
        .map_err(|e| format!("Failed to read upload response: {e}"))?
        .trim()
        .to_string();

    if url.is_empty() || !url.starts_with("https://") {
        return Err(format!("Unexpected Litterbox response: {url:?}"));
    }

    Ok(UploadResult { url })
}

/// Download raw bytes from a Litterbox URL.
pub async fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download returned HTTP {} — code may have expired",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download body: {e}"))?;

    Ok(bytes.to_vec())
}

/// Extract the Litterbox file ID from a response URL.
/// e.g. "https://litter.catbox.moe/abc123.bin" → "abc123"
/// Returns None if the URL can't be parsed.
pub fn extract_file_id_from_url(url: &str) -> Option<String> {
    let filename = url.trim().rsplit('/').next()?;
    let id = filename.strip_suffix(".bin").unwrap_or(filename);
    if id.is_empty() { return None; }
    Some(id.to_lowercase())
}

/// Build a sync code from the Litterbox URL and the pre-generated key hex.
///
/// `https://litter.catbox.moe/abc123.bin` + key `A3F9B2`
/// → `ZEN-A3F9B2-ABC123`
pub fn url_to_sync_code(url: &str, key_hex: &str) -> Option<String> {
    let filename = url.trim().rsplit('/').next()?;
    let file_id = filename.strip_suffix(".bin").unwrap_or(filename);
    if file_id.is_empty() {
        return None;
    }
    Some(format!(
        "ZEN-{}-{}",
        key_hex.to_uppercase(),
        file_id.to_uppercase()
    ))
}

/// Reverse a sync code into `(key_hex, download_url)`.
///
/// `ZEN-A3F9B2-ABC123` → `("A3F9B2", "https://litter.catbox.moe/abc123.bin")`
pub fn parse_sync_code(code: &str) -> Option<(String, String)> {
    let rest = code.strip_prefix("ZEN-")?;
    let (key_hex, file_id) = rest.split_once('-')?;
    if key_hex.is_empty() || file_id.is_empty() {
        return None;
    }
    let url = format!(
        "https://litter.catbox.moe/{}.bin",
        file_id.to_lowercase()
    );
    Some((key_hex.to_string(), url))
}
