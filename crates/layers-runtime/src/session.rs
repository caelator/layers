//! Session lifecycle management: creation, routing, archival, daily/idle reset.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc, NaiveTime, Duration as ChronoDuration};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use layers_core::{
    Session, SessionFilter, SessionStore,
    DmScope, Result, LayersError,
};

// ---------------------------------------------------------------------------
// DM scope routing
// ---------------------------------------------------------------------------

/// How DM sessions are scoped for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum DmScopeMode {
    /// One session per agent (global).
    Main,
    /// One session per peer across all channels.
    PerPeer,
    /// One session per (channel, peer) pair.
    PerChannelPeer,
    /// One session per (account, channel, peer) tuple.
    PerAccountChannelPeer,
}


/// Inputs used to derive the session routing key.
#[derive(Debug, Clone)]
pub struct SessionRouting {
    pub agent_id: String,
    pub channel: Option<String>,
    pub peer_id: Option<String>,
    pub account_id: Option<String>,
    pub thread_id: Option<String>,
}

impl SessionRouting {
    /// Build the canonical session key from routing inputs + scope mode.
    pub fn session_key(&self, scope: DmScopeMode) -> String {
        let main = match scope {
            DmScopeMode::Main => "main".to_string(),
            DmScopeMode::PerPeer => {
                format!("peer:{}", self.peer_id.as_deref().unwrap_or("unknown"))
            }
            DmScopeMode::PerChannelPeer => {
                let ch = self.channel.as_deref().unwrap_or("default");
                let peer = self.peer_id.as_deref().unwrap_or("unknown");
                format!("ch:{ch}:peer:{peer}")
            }
            DmScopeMode::PerAccountChannelPeer => {
                let acct = self.account_id.as_deref().unwrap_or("default");
                let ch = self.channel.as_deref().unwrap_or("default");
                let peer = self.peer_id.as_deref().unwrap_or("unknown");
                format!("acct:{acct}:ch:{ch}:peer:{peer}")
            }
        };

        if let Some(tid) = &self.thread_id {
            format!("agent:{}:{}:thread:{}", self.agent_id, main, tid)
        } else {
            format!("agent:{}:{}", self.agent_id, main)
        }
    }
}

// ---------------------------------------------------------------------------
// Session reset policies
// ---------------------------------------------------------------------------

/// Configuration for automatic session resets.
#[derive(Debug, Clone)]
pub struct ResetPolicy {
    /// Hour of day (0-23) at which daily reset triggers. Default: 4 (4 AM).
    pub daily_reset_hour: u32,
    /// Timezone name for daily reset (defaults to UTC).
    pub timezone: String,
    /// Idle duration (seconds) after which session resets. None = disabled.
    pub idle_timeout_secs: Option<u64>,
}

impl Default for ResetPolicy {
    fn default() -> Self {
        Self {
            daily_reset_hour: 4,
            timezone: "UTC".to_string(),
            idle_timeout_secs: None,
        }
    }
}

// ---------------------------------------------------------------------------
// SessionManager
// ---------------------------------------------------------------------------

/// Manages session lifecycle: creation, resolution, archival, reset.
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    dm_scope: DmScopeMode,
    reset_policy: ResetPolicy,
    /// In-memory cache of active session keys → session IDs.
    active: RwLock<HashMap<String, String>>,
}

impl SessionManager {
    pub fn new(
        store: Arc<dyn SessionStore>,
        dm_scope: DmScopeMode,
        reset_policy: ResetPolicy,
    ) -> Self {
        Self {
            store,
            dm_scope,
            reset_policy,
            active: RwLock::new(HashMap::new()),
        }
    }

    /// Resolve or create a session for the given routing inputs.
    /// Handles daily reset and idle reset transparently.
    pub async fn resolve_session(&self, routing: &SessionRouting) -> Result<Session> {
        let key = routing.session_key(self.dm_scope);

        // Check in-memory cache first.
        {
            let cache = self.active.read().await;
            if let Some(session_id) = cache.get(&key) {
                match self.store.get(session_id).await {
                    Ok(session) => {
                        if self.needs_reset(&session) {
                            debug!(session_id = %session.id, "session needs reset");
                            // Fall through to reset path below.
                        } else {
                            return Ok(session);
                        }
                    }
                    Err(LayersError::SessionNotFound(_)) => {
                        // Cache is stale, fall through to creation.
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        // Try to find existing session via store listing.
        let filter = SessionFilter {
            agent_id: Some(routing.agent_id.clone()),
            channel: routing.channel.clone(),
            peer_id: routing.peer_id.clone(),
            since: None,
        };

        let existing = self.store.list(&filter).await?;
        if let Some(session) = existing.into_iter().next() {
            if self.needs_reset(&session) {
                return self.reset_session(&key, &session).await;
            }
            // Cache it.
            self.active.write().await.insert(key, session.id.clone());
            return Ok(session);
        }

        // No existing session — create a fresh one.
        self.create_session(routing, &key).await
    }

    /// Create a brand-new session.
    async fn create_session(
        &self,
        routing: &SessionRouting,
        key: &str,
    ) -> Result<Session> {
        let now = Utc::now();
        let session = Session {
            id: Uuid::new_v4().to_string(),
            agent_id: routing.agent_id.clone(),
            dm_scope: routing.peer_id.as_ref().map(|pid| DmScope {
                channel: routing.channel.clone().unwrap_or_default(),
                peer_id: pid.clone(),
            }),
            thread_binding: None,
            created_at: now,
            updated_at: now,
            model: None,
            metadata: HashMap::new(),
            message_count: 0,
            token_count: 0,
        };

        self.store.put(&session).await?;
        self.active.write().await.insert(key.to_string(), session.id.clone());
        info!(session_id = %session.id, key = %key, "created new session");
        Ok(session)
    }

    /// Archive current session and create a fresh one with the same routing.
    pub async fn manual_reset(
        &self,
        routing: &SessionRouting,
    ) -> Result<Session> {
        let key = routing.session_key(self.dm_scope);
        let cache = self.active.read().await;
        if let Some(session_id) = cache.get(&key) {
            let session = self.store.get(session_id).await.ok();
            drop(cache);
            if let Some(s) = session {
                return self.reset_session(&key, &s).await;
            }
        } else {
            drop(cache);
        }
        // Nothing to reset — just create fresh.
        self.create_session(routing, &key).await
    }

    /// Archive `old` session, create a fresh replacement.
    async fn reset_session(&self, key: &str, old: &Session) -> Result<Session> {
        info!(session_id = %old.id, "archiving session for reset");
        // Archive by adding metadata marker and keeping in store.
        let mut archived = old.clone();
        archived.metadata.insert(
            "archived_at".to_string(),
            serde_json::Value::String(Utc::now().to_rfc3339()),
        );
        self.store.put(&archived).await?;

        // Build routing from old session to create replacement.
        let routing = SessionRouting {
            agent_id: old.agent_id.clone(),
            channel: old.dm_scope.as_ref().map(|d| d.channel.clone()),
            peer_id: old.dm_scope.as_ref().map(|d| d.peer_id.clone()),
            account_id: None,
            thread_id: old.thread_binding.as_ref().map(|t| t.thread_id.clone()),
        };

        let now = Utc::now();
        let session = Session {
            id: Uuid::new_v4().to_string(),
            agent_id: routing.agent_id.clone(),
            dm_scope: old.dm_scope.clone(),
            thread_binding: old.thread_binding.clone(),
            created_at: now,
            updated_at: now,
            model: old.model.clone(),
            metadata: HashMap::new(),
            message_count: 0,
            token_count: 0,
        };

        self.store.put(&session).await?;
        self.active.write().await.insert(key.to_string(), session.id.clone());
        info!(session_id = %session.id, "created replacement session after reset");
        Ok(session)
    }

    /// Check whether a session needs automatic reset (daily or idle).
    fn needs_reset(&self, session: &Session) -> bool {
        let now = Utc::now();

        // Daily reset: session created before today's reset hour.
        let reset_hour = self.reset_policy.daily_reset_hour;
        let today_reset = now.date_naive()
            .and_time(NaiveTime::from_hms_opt(reset_hour, 0, 0).unwrap_or_default());
        let today_reset_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_reset, Utc);

        if session.created_at < today_reset_utc && now >= today_reset_utc {
            return true;
        }

        // Idle reset.
        if let Some(idle_secs) = self.reset_policy.idle_timeout_secs {
            let idle_dur = ChronoDuration::seconds(idle_secs as i64);
            if now - session.updated_at > idle_dur {
                return true;
            }
        }

        false
    }

    /// Get the underlying store reference.
    pub fn store(&self) -> &dyn SessionStore {
        &*self.store
    }
}
