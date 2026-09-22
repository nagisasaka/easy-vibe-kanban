//! Explicit write intent for fields which are never echoed by discovery.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One process-local gate for infrequent native settings mutations, shared by
/// typed settings, tools and the legacy advanced MCP editor. External editors
/// do not participate; their observed revisions are checked before replacement.
pub static NATIVE_SETTINGS_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SensitiveWrite<T> {
    Preserve,
    Replace { value: T },
    Clear,
}

impl<T: Clone + Default> SensitiveWrite<T> {
    pub fn resolve(self, current: Option<&T>) -> Result<T, &'static str> {
        match self {
            Self::Preserve => current.cloned().ok_or(
                "Preserve requires an existing value; explicitly replace or clear new fields",
            ),
            Self::Replace { value } => Ok(value),
            Self::Clear => Ok(T::default()),
        }
    }
}
