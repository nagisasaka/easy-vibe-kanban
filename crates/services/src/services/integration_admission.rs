//! Short single-service admission barrier for filesystem mutations which cannot
//! participate in a SQLite transaction (manual Git/cleanup) and the in-memory
//! follow-up queue. NOT a business reservation or a model-execution lease.
//! Never hold this across model execution or host validation commands.
pub static MUTATIONS: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));
