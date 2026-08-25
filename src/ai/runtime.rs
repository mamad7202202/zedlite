//! Background tokio runtime shared by the whole app, plus HTTP client
//! construction with optional proxy support.

use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Process-wide multi-threaded tokio runtime. GPUI owns the UI thread; every
/// blocking/async AI and terminal task runs here instead.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .expect("failed to start tokio runtime")
    })
}

pub fn handle() -> tokio::runtime::Handle {
    runtime().handle().clone()
}

/// Build a reqwest client, optionally routed through a proxy endpoint.
pub fn build_client(proxy_url: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut b = reqwest::Client::builder();
    if let Some(url) = proxy_url {
        if !url.trim().is_empty() {
            b = b.proxy(reqwest::Proxy::all(url.trim())?);
        }
    }
    Ok(b.build()?)
}
