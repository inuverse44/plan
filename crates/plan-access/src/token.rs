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

/// Reuse a token for 45 minutes (Google tokens expire after an hour)
pub struct Tokens {
    src: TokenSource,
    cache: Mutex<Option<(String, Instant)>>,
}

impl Tokens {
    pub fn new(src: TokenSource) -> Self { Tokens { src, cache: Mutex::new(None) } }

    pub fn get(&self) -> Result<String, Vec<Issue>> {
        let mut c = self.cache.lock().unwrap();
        if let Some((t, at)) = c.as_ref() { if at.elapsed() < Duration::from_secs(45 * 60) { return Ok(t.clone()); } }
        let t = match &self.src {
            TokenSource::Metadata => {
                let v: serde_json::Value = ureq::get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
                    .set("Metadata-Flavor", "Google").call().map_err(e)?.into_json().map_err(e)?;
                v["access_token"].as_str().ok_or_else(|| e("the metadata server returned no token"))?.to_string()
            }
            TokenSource::Command(cmd) => {
                let out = std::process::Command::new("sh").arg("-c").arg(cmd).output().map_err(e)?;
                if !out.status.success() { return Err(e(format!("cannot get a token ({cmd}): {}", String::from_utf8_lossy(&out.stderr).trim()))); }
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
        };
        *c = Some((t.clone(), Instant::now()));
        Ok(t)
    }
}

/// Whether we run on Cloud Run (it sets K_SERVICE)
pub fn on_cloud_run() -> bool { std::env::var("K_SERVICE").is_ok() }
