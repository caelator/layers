# RFC-007: Elevate RLEF to First-Class Dispatch + OpenRouter Free Lane + OAuth2 Auth

**Date:** 2026-04-10  
**Status:** Draft  
**Author:** Caelator  

## Summary

Three changes in one RFC because they're deeply intertwined:

1. **Promote RLEF** from a standalone plugin to the integrated multi-provider dispatch layer
2. **Add OpenRouter** as a zero-cost "frontier and free" provider lane (Qwen 3.6 Plus, etc.)
3. **Add OAuth2** as a first-class auth strategy (starting with Minimax, reusable for any provider)

## Motivation

RLEF (Runtime Learned Expression Framework) is a well-built Coulomb-repulsion diversity selector sitting unused in `src/plugins/rlef/`. The main router (`router.rs`) does classification and route corrections but has no multi-provider dispatch. Meanwhile, OpenRouter offers free-tier frontier models, and Minimax supports OAuth — neither is wired in.

We need a dispatch layer that:
- Selects among multiple model providers with diversity (RLEF)
- Supports zero-cost lanes for cost-sensitive work (OpenRouter free pool)
- Supports OAuth2 for providers that require it (Minimax)
- Circuit-breakers on failures, integrates with existing route-correction feedback

## Architecture

### 1. Provider Trait

```rust
/// A model provider that can serve completion requests.
pub trait Provider: Send + Sync {
    /// Unique identifier for this provider (e.g., "openrouter", "minimax").
    fn id(&self) -> &str;
    
    /// The models available through this provider.
    fn models(&self) -> &[ModelSpec];
    
    /// Whether this provider requires OAuth authentication.
    fn auth_kind(&self) -> AuthKind;
    
    /// Estimate cost per 1K tokens. Zero for free-tier providers.
    fn cost_per_1k(&self) -> f64;
    
    /// Execute a completion request.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, ProviderError>;
}

pub enum AuthKind {
    ApiKey,
    OAuth2(OAuth2Config),
}

pub struct ModelSpec {
    pub model_id: String,        // e.g., "qwen/qwen3.6-plus"
    pub context_window: usize,
    pub supports_tools: bool,
    pub supports_streaming: bool,
}
```

### 2. RLEF Dispatch Elevation

Currently `RlefRouterPlugin` selects among abstract string labels. Elevate it to select among `Provider` instances:

```rust
pub struct RlefDispatcher {
    inner: RlefRouterPlugin,
    providers: Vec<Box<dyn Provider>>,
}

impl RlefDispatcher {
    /// Select a provider for the given request using Coulomb-repulsion diversity.
    pub fn select_provider(&mut self, request: &CompletionRequest) -> &dyn Provider {
        let candidates: Vec<&str> = self.providers.iter().map(|p| p.id()).collect();
        let chosen = self.inner.select(&candidates);
        // ... return the matching provider
    }
    
    /// Record that a provider was used (increases its charge).
    pub fn record_dispatch(&mut self, provider_id: &str) {
        self.inner.record_selection(provider_id);
    }
}
```

The dispatcher sits **between** the classifier (which decides *what kind* of work) and the provider (which does the work). Route corrections from `router.rs` feed back into charge adjustments — a corrected provider gets extra charge, naturally pushing dispatch elsewhere.

### 3. OpenRouter Provider

```rust
pub struct OpenRouterProvider {
    client: reqwest::Client,
    api_key: String,
    /// Only use models in the free pool.
    free_only: bool,
}

// OpenRouter is OpenAI-compatible — hit https://openrouter.ai/api/v1/chat/completions
// Enforce free_only by filtering models to those with pricing.prompt == "0"
```

Initial model roster:
- `qwen/qwen3.6-plus` — primary free frontier
- Others added as discovered/configurable

### 4. OAuth2 Auth Strategy

```rust
pub struct OAuth2Config {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: Option<String>,
    /// Grant type: client_credentials (for machine-to-machine) or authorization_code (for user-facing)
    pub grant_type: OAuth2GrantType,
}

pub enum OAuth2GrantType {
    ClientCredentials,
    AuthorizationCode { redirect_uri: String },
}

/// Manages OAuth2 token lifecycle (acquire, cache, refresh).
pub struct OAuth2TokenManager {
    config: OAuth2Config,
    cached_token: Mutex<Option<TokenWithExpiry>>,
}

impl OAuth2TokenManager {
    /// Get a valid token, refreshing if expired.
    pub async fn token(&self) -> Result<String, AuthError>;
}
```

Minimax implementation uses `ClientCredentials` grant — machine-to-machine, no user browser needed.

### 5. File Structure

```
src/
  dispatch/
    mod.rs              — RlefDispatcher, DispatchResult
    provider.rs         — Provider trait, ModelSpec, AuthKind
    openrouter.rs       — OpenRouterProvider (OpenAI-compatible, free-tier enforcement)
    minimax.rs          — MinimaxProvider (OAuth2-authenticated)
  auth/
    mod.rs              — AuthKind, AuthError
    oauth2.rs           — OAuth2Config, OAuth2TokenManager
  plugins/
    rlef/               — UNCHANGED (RlefRouterPlugin stays as-is, RlefDispatcher wraps it)
  router.rs             — UPDATED: feeds route corrections into dispatcher charge adjustments
```

### 6. Integration Points

- **Route corrections** (`router.rs`): When a correction is recorded, the dispatcher adds extra charge to the failed provider, naturally demoting it via Coulomb repulsion.
- **Circuit breaker** (`emit_failure` from RFC-006): A `ProviderError` triggers `emit_failure()` with the provider ID as the lane. Existing circuit-breaker logic handles cooldown.
- **Config**: New section in `.layers/config.toml`:

```toml
[dispatch]
default_provider = "openrouter"

[dispatch.providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
free_only = true
models = ["qwen/qwen3.6-plus"]

[dispatch.providers.minimax]
auth = "oauth2"
oauth2_token_url = "https://api.minimaxi.chat/v1/oauth/token"
oauth2_client_id_env = "MINIMAX_CLIENT_ID"
oauth2_client_secret_env = "MINIMAX_CLIENT_SECRET"
```

## Migration Path

1. **Phase 1**: Add `dispatch/`, `auth/` modules. Wire `RlefDispatcher` wrapping existing `RlefRouterPlugin`. OpenRouter provider with API key auth only.
2. **Phase 2**: Add OAuth2 module. Minimax provider with `ClientCredentials` flow.
3. **Phase 3**: Wire route-correction feedback into dispatcher charges. Circuit-breaker integration.
4. **Phase 4**: Persist dispatcher state (charges) to disk via existing RLEF serde support.

## Testing Strategy

- Unit tests for `RlefDispatcher` (provider selection, charge tracking)
- Unit tests for `OpenRouterProvider` with mock HTTP (free-tier filtering)
- Unit tests for `OAuth2TokenManager` (token acquisition, refresh, expiry)
- Integration test: full dispatch cycle with mock providers
- Live smoke test: `layers query` routed through OpenRouter free tier

## Open Questions

- Should the dispatcher also consider request properties (e.g., tool-use requests go only to providers with `supports_tools: true`)?
- What's the fallback when all free-tier providers are circuit-broken — fail open or fail closed?
- Should OAuth2 tokens be persisted to disk to survive process restarts, or always re-acquire?
