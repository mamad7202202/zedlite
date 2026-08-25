//! Ported from dragon-agent core (crates/core/src/config.rs) and extended with
//! proxy routing so each outbound service can independently use the proxy.
//!
//! Configuration: TOML on disk, BYOK providers, model resolution.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Model spec like "openrouter/anthropic/claude-sonnet-4".
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub providers: Vec<ProviderCfg>,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCfg {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    /// "openai" or "anthropic". Auto-detected from base_url when omitted.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// When history grows past this many entries it gets compacted into a summary.
    #[serde(default = "default_compaction")]
    pub compaction_messages: usize,
    /// Allow the agent to run shell commands via its run_shell tool.
    #[serde(default)]
    pub allow_commands: bool,
    /// Approval patterns that skip the prompt: "write_file" or "run_shell:npm".
    #[serde(default)]
    pub auto_approve: Vec<String>,
    /// UI theme: "dark" | "light".
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Mode used when a session starts.
    #[serde(default = "default_mode")]
    pub default_mode: String,
    /// Deep-thinking effort passed to models that support it:
    /// "off" | "low" | "medium" | "high".
    #[serde(default = "default_thinking")]
    pub thinking: String,
    /// Memory engine: classic hybrid facts ("hybrid") or the memory graph ("graph").
    #[serde(default = "default_engine")]
    pub memory_engine: String,
    // ------------------------------------------------------------- proxy
    /// Shared proxy endpoint, e.g. "http://127.0.0.1:8080" or "socks5://...".
    /// Empty string = no proxy configured at all.
    #[serde(default)]
    pub proxy_url: String,
    /// Route LLM API requests through the proxy.
    #[serde(default)]
    pub proxy_llm: bool,
    /// Route the agent's `web_search` tool through the proxy.
    #[serde(default)]
    pub proxy_web_search: bool,
    /// Route the agent's `fetch_url` tool through the proxy.
    #[serde(default)]
    pub proxy_fetch: bool,
}

fn default_compaction() -> usize {
    36
}
fn default_theme() -> String {
    "dark".into()
}
fn default_mode() -> String {
    "agent".into()
}
fn default_thinking() -> String {
    "off".into()
}
fn default_engine() -> String {
    "hybrid".into()
}

impl Settings {
    /// Normalized thinking level, ignoring unknown values.
    pub fn thinking_level(&self) -> Thinking {
        match self.thinking.as_str() {
            "low" => Thinking::Low,
            "medium" => Thinking::Medium,
            "high" => Thinking::High,
            _ => Thinking::Off,
        }
    }
    pub fn graph_memory(&self) -> bool {
        self.memory_engine == "graph"
    }
    pub fn proxy_active(&self) -> bool {
        !self.proxy_url.trim().is_empty()
    }
    /// Which services should go through the proxy right now.
    pub fn proxy_routes(&self) -> ProxyRoutes {
        ProxyRoutes {
            llm: self.proxy_active() && self.proxy_llm,
            web_search: self.proxy_active() && self.proxy_web_search,
            fetch: self.proxy_active() && self.proxy_fetch,
        }
    }
}

/// Per-service proxy routing decisions, resolved once per config load.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProxyRoutes {
    pub llm: bool,
    pub web_search: bool,
    pub fetch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thinking {
    Off,
    Low,
    Medium,
    High,
}

impl Default for ProviderCfg {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            kind: None,
            models: Vec::new(),
        }
    }
}

impl Config {
    pub fn dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zedlite")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    /// Where sessions and memory live.
    pub fn data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zedlite")
    }

    pub fn load() -> Result<Self> {
        let p = Self::path();
        if !p.exists() {
            return Ok(Config::default());
        }
        let raw =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing {}", p.display()))
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(Self::dir())?;
        std::fs::write(Self::path(), toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// A fully-commented starter config written on first "Edit Config".
    pub fn template() -> String {
        r#"# ZedLite configuration - edit and hit Ctrl+S in this tab to apply.
# This file is re-loaded live whenever you save it.

# Active model as "provider/model-id":
default_model = "openai/gpt-4o-mini"

[[providers]]
name = "openai"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."
kind = "openai"              # openai | anthropic (auto-detected when omitted)
models = ["gpt-4o-mini", "gpt-4o"]

# Any OpenAI-compatible endpoint works the same way:
# [[providers]]
# name = "openrouter"
# base_url = "https://openrouter.ai/api/v1"
# api_key = "sk-or-..."
# models = ["anthropic/claude-sonnet-4", "meta-llama/llama-3.1-70b-instruct"]

# [[providers]]
# name = "ollama-local"
# base_url = "http://localhost:11434/v1"
# api_key = "ollama"
# models = ["llama3.1", "qwen2.5-coder:7b"]

[settings]
compaction_messages = 36
allow_commands = true
auto_approve = []              # e.g. ["write_file", "run_shell:cargo"]
theme = "dark"
default_mode = "agent"         # chat | plan | agent
thinking = "off"               # off | low | medium | high
memory_engine = "hybrid"       # hybrid | graph

# ---- proxy -----------------------------------------------------------------
# Requests can be routed user -> proxy -> service. Each service is opt-in:
proxy_url = ""                 # "" disables; e.g. "http://127.0.0.1:8080"
proxy_llm = false              # send model API traffic through the proxy
proxy_web_search = false       # send the web_search tool through the proxy
proxy_fetch = false            # send the fetch_url tool through the proxy
"#
        .to_string()
    }

    pub fn find_provider(&self, name: &str) -> Option<&ProviderCfg> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// Resolve a model spec ("provider/model", possibly with more slashes) to
    /// a concrete provider + model id. `None` falls back to default_model,
    /// then to the single configured provider if there is exactly one.
    pub fn resolve_model(&self, spec: Option<&str>) -> Result<(&ProviderCfg, String)> {
        let spec = spec.or(self.default_model.as_deref());

        let (prov_name, model_id) = match spec {
            Some(s) => match s.split_once('/') {
                Some((p, m)) => (p.to_string(), m.to_string()),
                None => {
                    if self.providers.len() == 1 {
                        (self.providers[0].name.clone(), s.to_string())
                    } else {
                        bail!(
                            "model '{s}' has no provider prefix; use 'provider/model' \
                             (configured providers: {})",
                            self.provider_names().join(", ")
                        );
                    }
                }
            },
            None => {
                if let Some(p) = self.providers.first() {
                    bail!(
                        "no default model set; try setting default_model = \"{}/{}\"",
                        p.name,
                        p.models.first().cloned().unwrap_or_default()
                    );
                }
                bail!(
                    "no models configured yet.\n\nOpen Settings in the toolbar \
                     (or Ctrl+Alt+,) and add your first provider:\n\
                     [[providers]]\nname = \"openai\"\nbase_url = \"https://api.openai.com/v1\"\n\
                     api_key = \"sk-...\"\nmodels = [\"gpt-4o-mini\"]\nthen set \
                     default_model = \"openai/gpt-4o-mini\"\n\nAnything \
                     OpenAI-compatible works (OpenRouter, Groq, Ollama, LM Studio)."
                );
            }
        };

        let prov = self
            .find_provider(&prov_name)
            .with_context(|| format!("provider '{prov_name}' not found"))?;
        Ok((prov, model_id))
    }

    pub fn provider_names(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.name.clone()).collect()
    }
}
