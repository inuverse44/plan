//! WebClient の入れ物：画面のファイル（web/）をバイナリに埋め込み、手元のサーバー用と書き出し用の HTML を作る
use base64::Engine;

pub const INDEX: &str = include_str!("../../../web/index.html");
/// 読む順に並べる（後のファイルが前のファイルの関数を使う）
pub const SCRIPTS: &[(&str, &str)] = &[
    ("pkg/plan_web.js", include_str!("../../../web/pkg/plan_web.js")),
    ("ui/core.js", include_str!("../../../web/ui/core.js")),
    ("ui/gantt.js", include_str!("../../../web/ui/gantt.js")),
    ("ui/network.js", include_str!("../../../web/ui/network.js")),
    ("ui/load.js", include_str!("../../../web/ui/load.js")),
    ("ui/table.js", include_str!("../../../web/ui/table.js")),
    ("ui/app.js", include_str!("../../../web/ui/app.js")),
];
pub const WASM: &[u8] = include_bytes!("../../../web/pkg/plan_web_bg.wasm");

fn page(scripts: String, boot: serde_json::Value) -> String {
    let boot = boot.to_string().replace("</", "<\\/");
    INDEX.replace("<!--SCRIPTS-->", &format!("<script id=\"boot\" type=\"application/json\">{boot}</script>\n{scripts}"))
}

pub fn live_page() -> String {
    page(SCRIPTS.iter().map(|(p, _)| format!("<script src=\"/{p}\"></script>")).collect::<Vec<_>>().join("\n"), serde_json::json!({ "mode": "live" }))
}

pub fn export_page(plan: &plan_contracts::Plan) -> String {
    let wasm = base64::engine::general_purpose::STANDARD.encode(WASM);
    let scripts = SCRIPTS.iter().map(|(_, s)| format!("<script>{}</script>", s.replace("</script", "<\\/script"))).collect::<Vec<_>>().join("\n");
    page(scripts, serde_json::json!({ "mode": "export", "plan": plan, "wasm": wasm }))
}

pub fn asset(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let p = path.trim_start_matches('/');
    if p == "pkg/plan_web_bg.wasm" { return Some(("application/wasm", WASM.to_vec())); }
    SCRIPTS.iter().find(|(n, _)| *n == p).map(|(_, s)| ("text/javascript; charset=utf-8", s.as_bytes().to_vec()))
}
