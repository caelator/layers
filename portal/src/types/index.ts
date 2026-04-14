export interface Session {
  id: string;
  agent_id: string;
  created_at: string;
  updated_at: string;
  model: string | null;
  metadata: Record<string, unknown>;
  message_count: number;
  token_count: number;
}

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: string;
  attachments?: Attachment[];
}

export interface Attachment {
  file_id: string;
  name: string;
  size: number;
  type: string;
  url?: string;
  preview_url?: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
}

export interface ChannelHealth {
  name: string;
  health: string;
}

export interface DaemonStatus {
  uptime_secs: number;
  channels: ChannelHealth[];
}

export interface HealthResponse {
  status: string;
  version: string;
}

export interface Provider {
  name: string;
  api_base: string;
  api_key_set: boolean;
  models: string[];
  status: 'connected' | 'error' | 'untested';
}

export interface McpServer {
  name: string;
  url: string;
  api_key_set: boolean;
  tools: string[];
  status: 'connected' | 'error' | 'untested';
}

export interface ModelConfig {
  temperature: number;
  max_tokens: number;
  top_p: number;
  frequency_penalty: number;
  presence_penalty: number;
  context_window: number;
  thinking: 'off' | 'on' | 'stream';
}

export interface ConfigPreset {
  id: string;
  name: string;
  model_id: string;
  config: ModelConfig;
  system_prompt?: string;
}

export interface DaemonInfo {
  bind_address: string;
  port: number;
  tls_enabled: boolean;
  pid?: number;
  uptime_secs?: number;
}
