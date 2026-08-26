//! ZedLite AI hub — glue between the editor UI (GPUI) and the ported
//! dragon-agent core.
//!
//! Everything the panels need lives here:
//! - one [`AiHub`] shared state object (config, agent, memory, sessions)
//! - a single event channel pumped from tokio into GPUI ([`UiEvent`])
//! - streaming terminal command execution
//! - AI inline code suggestions for the editor

pub mod agent;
pub mod config;
pub mod memory;
pub mod provider;
pub mod runtime;
pub mod session;

use anyhow::{Context as _, Result};
use tokio::io::AsyncBufReadExt;
use futures::channel::mpsc::UnboundedSender;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use agent::{Agent, AgentEvent, Mode};
pub use config::Config;
pub use memory::graph::GraphStore;
pub use memory::MemoryStore;
pub use session::SessionLog;

/// Every async outcome the UI cares about flows through this enum.
#[derive(Debug)]
pub enum UiEvent {
    // chat / agent
    Delta(String),
    ToolStart { name: String, detail: String },
    ToolEnd { name: String },
    ApprovalRequest { id: u64, tool: String, detail: String },
    Usage { prompt: u64, completion: u64, total: u64 },
    Tasks(serde_json::Value),
    Compacted,
    Stopped,
    Error(String),
    Done(String),
    // terminal panel
    TerminalOut(String),
    TerminalExit(Option<i32>),
    // inline AI suggestion for the editor
    AiGhost { text: String },
}

pub struct AiHub {
    pub cfg: Mutex<Config>,
    pub agent: Arc<tokio::sync::Mutex<Option<Agent>>>,
    pub memory: Arc<Mutex<MemoryStore>>,
    pub graph: Arc<Mutex<GraphStore>>,
    pub session: Mutex<Option<SessionLog>>,
    /// Running terminal child process, kept so the Stop button can kill it.
    pub term_child: Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
    /// Root used by tools and the system prompt; updated when a folder opens.
    pub root: Mutex<PathBuf>,
    pub busy: AtomicBool,
    http_direct: Mutex<reqwest::Client>,
    http_proxy: Mutex<Option<reqwest::Client>>,
}

impl AiHub {
    pub fn new() -> Result<Arc<Self>> {
        let cfg = Config::load().unwrap_or_default();
        let http_direct = runtime::build_client(None)?;
        let proxy_url = cfg.settings.proxy_url.trim().to_string();
        let http_proxy = if proxy_url.is_empty() {
            None
        } else {
            Some(runtime::build_client(Some(&proxy_url)).context("building proxy client")?)
        };
        Ok(Arc::new(Self {
            cfg: Mutex::new(cfg),
            agent: Arc::new(tokio::sync::Mutex::new(None)),
            memory: Arc::new(Mutex::new(MemoryStore::open()?)),
            graph: Arc::new(Mutex::new(GraphStore::open()?)),
            session: Mutex::new(None),
            term_child: Arc::new(tokio::sync::Mutex::new(None)),
            root: Mutex::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
            busy: AtomicBool::new(false),
            http_direct: Mutex::new(http_direct),
            http_proxy: Mutex::new(http_proxy),
        }))
    }

    pub fn set_root(&self, path: PathBuf) {
        *self.root.lock().unwrap() = path;
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    /// Re-read config.toml from disk, refresh HTTP clients and rebuild the
    /// agent with the new model/proxy settings. Called when the user saves
    /// the config tab or changes settings through the panels.
    pub fn reload_config(&self) -> Result<String> {
        let cfg = Config::load()?;
        let summary = format!(
            "config reloaded · model: {} · providers: {} · engine: {}{}",
            cfg.default_model.as_deref().unwrap_or("(none set)"),
            if cfg.providers.is_empty() {
                "0".to_string()
            } else {
                cfg.provider_names().join(",")
            },
            cfg.settings.memory_engine,
            if cfg.settings.proxy_active() {
                format!(
                    " · proxy {} (llm={} web={} fetch={})",
                    cfg.settings.proxy_url,
                    cfg.settings.proxy_llm,
                    cfg.settings.proxy_web_search,
                    cfg.settings.proxy_fetch
                )
            } else {
                String::new()
            }
        );
        let proxy_url = cfg.settings.proxy_url.trim().to_string();
        *self.http_direct.lock().unwrap() = runtime::build_client(None)?;
        *self.http_proxy.lock().unwrap() = if proxy_url.is_empty() {
            None
        } else {
            Some(runtime::build_client(Some(&proxy_url)).context("building proxy client")?)
        };
        {
            *self.cfg.lock().unwrap() = cfg.clone();
        }
        self.rebuild_agent(&cfg)?;
        Ok(summary)
    }

    /// (Re)build the underlying agent from the given config snapshot.
    fn rebuild_agent(&self, cfg: &Config) -> Result<()> {
        let routes = cfg.settings.proxy_routes();
        let (prov_cfg, model) = cfg.resolve_model(None)?;
        let direct = self.http_direct.lock().unwrap().clone();
        let proxy = self.http_proxy.lock().unwrap().clone();
        let prov = provider::build(prov_cfg, direct.clone(), proxy.clone(), routes)?;
        let allow_commands = cfg.settings.allow_commands;
        let compaction = cfg.settings.compaction_messages;

        let mut agent = Agent::new(
            prov,
            model,
            self.memory.clone(),
            allow_commands,
            compaction,
            self.root.lock().unwrap().clone(),
            direct,
            proxy,
            routes.web_search,
            routes.fetch,
        );
        agent.set_mode(Mode::parse(&cfg.settings.default_mode).unwrap_or(Mode::Agent));
        agent.set_thinking(cfg.settings.thinking_level());
        agent.set_auto_approve(cfg.settings.auto_approve.clone());
        if cfg.settings.graph_memory() {
            agent.set_engine(Some(self.graph.clone()));
        }
        let sid = self.current_session_id();
        agent.set_session(sid.as_deref());

        *self.blocking_agent() = Some(agent);
        Ok(())
    }

    fn blocking_agent(&self) -> tokio::sync::MutexGuard<'_, Option<Agent>> {
        // Only ever called synchronously from the UI thread, never inside an
        // async context, so blocking on the tokio mutex is safe here.
        self.agent.blocking_lock()
    }

    pub fn current_session_id(&self) -> Option<String> {
        self.session.lock().unwrap().as_ref().map(|s| s.meta.id.clone())
    }

    /// Ensure an agent exists (first prompt / after config change).
    fn ensure_agent(&self) -> Result<()> {
        if self.blocking_agent().is_none() {
            let cfg = self.cfg.lock().unwrap().clone();
            self.rebuild_agent(&cfg)?;
        }
        Ok(())
    }

    /// Start a brand-new conversation (clears history + opens a fresh log).
    pub fn new_session(&self) -> Result<()> {
        if let Some(sid) = self.current_session_id() {
            let _ = self.memory.lock().unwrap().clear_session(&sid);
            self.graph.lock().unwrap().drop_session(&sid);
        }
        let model = self
            .cfg
            .lock()
            .unwrap()
            .default_model
            .clone()
            .unwrap_or_else(|| "unset".into());
        let log = SessionLog::create(&model)?;
        let sid = log.meta.id.clone();
        *self.session.lock().unwrap() = Some(log);
        if let Some(agent) = self.blocking_agent().as_mut() {
            agent.reset();
            agent.set_session(Some(&sid));
        }
        Ok(())
    }

    pub fn set_mode(&self, mode: Mode) {
        let _ = self.ensure_agent();
        if let Some(agent) = self.blocking_agent().as_mut() {
            agent.set_mode(mode);
        }
        let mut cfg = self.cfg.lock().unwrap();
        cfg.settings.default_mode = mode.as_str().to_string();
        let _ = cfg.save();
    }

    pub fn current_mode(&self) -> Mode {
        Mode::parse(&self.cfg.lock().unwrap().settings.default_mode).unwrap_or(Mode::Agent)
    }

    pub fn model_label(&self) -> String {
        self.cfg
            .lock()
            .unwrap()
            .default_model
            .clone()
            .unwrap_or_else(|| "(configure a model)".into())
    }

    pub fn stop(&self) {
        if let Ok(mut guard) = self.agent.try_lock() {
            if let Some(agent) = guard.as_ref() {
                agent.stop();
            }
        }
        self.busy.store(false, Ordering::Relaxed);
    }

    pub fn respond(&self, id: u64, allowed: bool) {
        if let Ok(guard) = self.agent.try_lock() {
            if let Some(agent) = guard.as_ref() {
                agent.respond(id, allowed);
            }
        }
    }

    /// Send a user prompt into the agent loop. Events stream back through
    /// `tx` so the GPUI task can paint them.
    pub fn send_prompt(self: &Arc<Self>, text: &str, tx: UnboundedSender<UiEvent>) {
        if text.trim().is_empty() {
            return;
        }
        if let Err(e) = self.ensure_agent() {
            let _ = tx.unbounded_send(UiEvent::Error(format!("{e:#}")));
            return;
        }
        if let Err(e) = self.open_session_and_log_user(text) {
            let _ = tx.unbounded_send(UiEvent::Error(format!("session: {e:#}")));
        }
        let hub = self.clone();
        let owned = text.to_string();
        self.busy.store(true, Ordering::Relaxed);

        runtime::runtime().spawn(async move {
            let (atx, mut arx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
            let job = {
                let agent = hub.agent.clone();
                let t = owned.clone();
                tokio::spawn(async move {
                    let mut g = agent.lock().await;
                    match g.as_mut() {
                        Some(a) => a.turn(&t, atx).await,
                        None => Err(anyhow::anyhow!("agent unavailable")),
                    }
                })
            };
            while let Some(ev) = arx.recv().await {
                let ui = match ev {
                    AgentEvent::Delta(d) => UiEvent::Delta(d),
                    AgentEvent::ToolStart { name, detail } => UiEvent::ToolStart { name, detail },
                    AgentEvent::ToolEnd { name } => UiEvent::ToolEnd { name },
                    AgentEvent::ApprovalRequest { id, tool, detail } => {
                        UiEvent::ApprovalRequest { id, tool, detail }
                    }
                    AgentEvent::Usage { prompt, completion, total } => {
                        UiEvent::Usage { prompt, completion, total }
                    }
                    AgentEvent::Tasks(v) => UiEvent::Tasks(v),
                    AgentEvent::Compacted => UiEvent::Compacted,
                    AgentEvent::Stopped => UiEvent::Stopped,
                    AgentEvent::Error(e) => UiEvent::Error(e),
                };
                if tx.unbounded_send(ui).is_err() {
                    break;
                }
            }
            let result = match job.await {
                Ok(Ok(text)) => Ok(text),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(e) => Err(format!("join error: {e}")),
            };
            // persist the assistant answer to the session transcript
            if let Ok(final_text) = &result {
                if !final_text.trim().is_empty() {
                    if let Some(log) = hub.session.lock().unwrap().as_mut() {
                        let _ =
                            log.append_message(&provider::Message::assistant(final_text.clone()));
                    }
                }
            }
            hub.busy.store(false, Ordering::Relaxed);
            let _ = tx.unbounded_send(match result {
                Ok(t) => UiEvent::Done(t),
                Err(e) => UiEvent::Error(e),
            });
        });
    }

    fn open_session_and_log_user(&self, text: &str) -> Result<()> {
        let needs_new = self.session.lock().unwrap().is_none();
        if needs_new {
            let model = self
                .cfg
                .lock()
                .unwrap()
                .default_model
                .clone()
                .unwrap_or_else(|| "unset".into());
            let log = SessionLog::create(&model)?;
            let sid = log.meta.id.clone();
            *self.session.lock().unwrap() = Some(log);
            if let Some(agent) = self.blocking_agent().as_mut() {
                agent.set_session(Some(&sid));
            }
        }
        if let Some(log) = self.session.lock().unwrap().as_mut() {
            log.append_message(&provider::Message::user(text))?;
            log.set_title_if_new(text);
        }
        Ok(())
    }

    // ------------------------------------------------------------- terminal

    /// Run a shell command in the workspace root, streaming combined output
    /// back as events. The child is stored in `term_child` so Stop can kill it.
    pub fn run_terminal_command(self: &Arc<Self>, command: &str, tx: UnboundedSender<UiEvent>) {
        let cmd = command.to_string();
        let cwd = self.root.lock().unwrap().clone();
        let slot = self.term_child.clone();

        runtime::runtime().spawn(async move {
            #[cfg(target_os = "windows")]
            let spawned = tokio::process::Command::new("cmd")
                .args(["/C", &cmd])
                .current_dir(&cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn();
            #[cfg(not(target_os = "windows"))]
            let spawned = tokio::process::Command::new("sh")
                .args(["-c", &cmd])
                .current_dir(&cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn();

            let mut child = match spawned {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.unbounded_send(UiEvent::TerminalOut(format!("[spawn error] {e}")));
                    let _ = tx.unbounded_send(UiEvent::TerminalExit(None));
                    return;
                }
            };

            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            let pump_out = async {
                if let Some(out) = stdout {
                    let mut reader = tokio::io::BufReader::new(out).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        if tx.unbounded_send(UiEvent::TerminalOut(line)).is_err() {
                            break;
                        }
                    }
                }
            };
            let pump_err = async {
                if let Some(err) = stderr {
                    let mut reader = tokio::io::BufReader::new(err).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        if tx.unbounded_send(UiEvent::TerminalOut(line)).is_err() {
                            break;
                        }
                    }
                }
            };

            {
                let mut guard = slot.lock().await;
                *guard = Some(child);
            }

            let (out_res, err_res, status) = tokio::join!(pump_out, pump_err, async {
                loop {
                    let mut guard = slot.lock().await;
                    if let Some(child) = guard.as_mut() {
                        match child.try_wait() {
                            Ok(Some(st)) => break Some(st),
                            Ok(None) => {}
                            Err(_) => break None,
                        }
                    } else {
                        break None;
                    }
                    drop(guard);
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                }
            });
            let _ = (out_res, err_res);

            let code = status.and_then(|s| s.code());
            {
                let mut guard = slot.lock().await;
                if let Some(mut child) = guard.take() {
                    // ensure reaping even when the streams closed early
                    let _ = child.wait().await;
                }
            }
            let _ = tx.unbounded_send(UiEvent::TerminalExit(code));
        });
    }

    /// Kill the running terminal command, if any.
    pub async fn kill_terminal(&self) {
        if let Some(child) = self.term_child.lock().await.as_mut() {
            let _ = child.kill().await;
        }
    }

    pub fn kill_terminal_sync(&self) {
        runtime::runtime().block_on(self.kill_terminal());
    }

    // -------------------------------------------------- AI inline suggestion

    /// Ask the configured model for an inline completion of the code prefix.
    /// The returned ghost text arrives as `UiEvent::AiGhost` on `tx`.
    pub fn request_inline_suggestion(
        self: &Arc<Self>,
        context_prefix: &str,
        suffix_line: &str,
        tx: UnboundedSender<UiEvent>,
    ) {
        let hub = self.clone();
        let prefix = context_prefix.to_string();
        let suffix = suffix_line.to_string();
        runtime::runtime().spawn(async move {
            let outcome = async {
                let cfg_snapshot = {
                    let cfg = hub.cfg.lock().unwrap();
                    cfg.clone()
                };
                let routes = cfg_snapshot.settings.proxy_routes();
                let (prov_cfg, model) = cfg_snapshot.resolve_model(None)?;
                let direct = hub.http_direct.lock().unwrap().clone();
                let proxy = hub.http_proxy.lock().unwrap().clone();
                let prov =
                    provider::build(prov_cfg, direct, proxy, routes)?;
                let system = "You are an inline code completion engine. Continue the code \
                              at <CURSOR>. Reply with ONLY the continuation text that fits on \
                              the current line - no explanations, no markdown fences, no \
                              repetition of existing code. Maximum ~40 characters.";
                let user = format!("{prefix}<CURSOR>{suffix}");
                provider::complete(
                    prov,
                    &model,
                    Some(system),
                    &[provider::Message::user(user)],
                )
                .await
            };
            match outcome.await {
                Ok(text) => {
                    let cleaned = clean_ghost(&text);
                    if !cleaned.is_empty() {
                        let _ = tx.unbounded_send(UiEvent::AiGhost { text: cleaned });
                    }
                }
                Err(e) => {
                    let _ = tx.unbounded_send(UiEvent::Error(format!("inline suggest: {e:#}")));
                }
            }
        });
    }
}

fn clean_ghost(raw: &str) -> String {
    let first_line = raw.lines().next().unwrap_or("").trim_end();
    first_line
        .replace("```", "")
        .replace("<CURSOR>", "")
        .chars()
        .take(60)
        .collect()
}
