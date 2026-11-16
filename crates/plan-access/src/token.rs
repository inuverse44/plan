//! Google のトークンの出どころ。Cloud Run の中ではメタデータサーバー、手元では gcloud のコマンド
use plan_contracts::Issue;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(crate) fn e(x: impl std::fmt::Display) -> Vec<Issue> { vec![Issue::error(x.to_string())] }

/// トークンを作る方法
pub enum TokenSource {
    /// Cloud Run の中：メタデータサーバーのアクセストークン
    Metadata,
    /// 手元：シェルのコマンド（例 `gcloud auth print-access-token`）の出力
    Command(String),
}

/// 作ったトークンを 45 分使い回す（Google のトークンは 1 時間で切れる）
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
                v["access_token"].as_str().ok_or_else(|| e("メタデータサーバーがトークンを返さない"))?.to_string()
            }
            TokenSource::Command(cmd) => {
                let out = std::process::Command::new("sh").arg("-c").arg(cmd).output().map_err(e)?;
                if !out.status.success() { return Err(e(format!("トークンを作れない（{cmd}）：{}", String::from_utf8_lossy(&out.stderr).trim()))); }
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
        };
        *c = Some((t.clone(), Instant::now()));
        Ok(t)
    }
}

/// Cloud Run の中で動いているか（Cloud Run は K_SERVICE を入れる）
pub fn on_cloud_run() -> bool { std::env::var("K_SERVICE").is_ok() }
