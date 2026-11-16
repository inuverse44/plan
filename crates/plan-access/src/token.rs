//! Where Google tokens come from: the metadata server on Cloud Run, a gcloud command locally
use plan_contracts::Issue;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(crate) fn e(x: impl std::fmt::Display) -> Vec<Issue> { vec![Issue::error(x.to_string())] }

/// How to get a token
pub enum TokenSource {
    /// On Cloud Run: an access token from the metadata server
    Metadata,
    /// Locally: the output of a shell command (e.g. `gcloud auth print-access-token`)
    Command(String),
}

/// Reuse a token until shortly before it expires.
/// The metadata server hands out a cached token whose remaining life can be well under an hour,
/// so its `expires_in` decides; a command's token is assumed to last an hour.
pub struct Tokens {
    src: TokenSource,
    cache: Mutex<Option<(String, Instant)>>,
}

/// Refresh this long before the token expires
const MARGIN: Duration = Duration::from_secs(5 * 60);

impl Tokens {
    pub fn new(src: TokenSource) -> Self { Tokens { src, cache: Mutex::new(None) } }

    pub fn get(&self) -> Result<String, Vec<Issue>> {
        let mut c = self.cache.lock().unwrap();
        if let Some((t, until)) = c.as_ref() { if Instant::now() < *until { return Ok(t.clone()); } }
        let (t, life) = match &self.src {
            TokenSource::Metadata => {
                let v: serde_json::Value = ureq::get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
                    .set("Metadata-Flavor", "Google").call().map_err(e)?.into_json().map_err(e)?;
                let t = v["access_token"].as_str().ok_or_else(|| e("the metadata server returned no token"))?.to_string();
                (t, Duration::from_secs(v["expires_in"].as_u64().unwrap_or(0)))
            }
            TokenSource::Command(cmd) => {
                let out = std::process::Command::new("sh").arg("-c").arg(cmd).output().map_err(e)?;
                if !out.status.success() { return Err(e(format!("cannot get a token ({cmd}): {}", String::from_utf8_lossy(&out.stderr).trim()))); }
                (String::from_utf8_lossy(&out.stdout).trim().to_string(), Duration::from_secs(60 * 60))
            }
        };
        *c = Some((t.clone(), Instant::now() + life.saturating_sub(MARGIN)));
        Ok(t)
    }

    /// Drop the cached token (after a 401), so the next `get` fetches a new one
    pub fn forget(&self) { *self.cache.lock().unwrap() = None; }
}

/// Whether we run on Cloud Run (it sets K_SERVICE)
pub fn on_cloud_run() -> bool { std::env::var("K_SERVICE").is_ok() }
