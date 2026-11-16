//! 資源を業務の動詞で出す。隠す変化は「計画の置き場と形式」と「暦の出どころ」
pub mod calendar_access;
pub mod plan_access;
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite;
#[cfg(not(target_arch = "wasm32"))]
pub mod token;
#[cfg(not(target_arch = "wasm32"))]
pub mod firestore;
#[cfg(not(target_arch = "wasm32"))]
pub mod http;
pub use calendar_access::{CalendarAccess, JpRuleCalendar};
pub use plan_access::{JsonPlan, PlanAccess, TomlText};
#[cfg(not(target_arch = "wasm32"))]
pub use plan_access::TomlFile;
#[cfg(not(target_arch = "wasm32"))]
pub use sqlite::SqlitePlans;
#[cfg(not(target_arch = "wasm32"))]
pub use firestore::FirestorePlans;
#[cfg(not(target_arch = "wasm32"))]
pub use http::HttpPlans;
#[cfg(not(target_arch = "wasm32"))]
pub use token::{on_cloud_run, TokenSource, Tokens};
