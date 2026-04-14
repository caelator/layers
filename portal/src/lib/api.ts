import type { Session, DaemonStatus, HealthResponse, ModelInfo, Provider, McpServer } from '../types';

const BASE = '';

export async function fetchHealth(): Promise<HealthResponse> {
  const res = await fetch(`${BASE}/health`);
  return res.json();
}

export async function fetchStatus(): Promise<DaemonStatus> {
  const res = await fetch(`${BASE}/api/status`);
  return res.json();
}

export async function fetchSessions(): Promise<Session[]> {
  const res = await fetch(`${BASE}/api/sessions`);
  const data = await res.json();
  return data.sessions ?? [];
}

export async function createSession(): Promise<Session> {
  const res = await fetch(`${BASE}/api/sessions`, { method: 'POST' });
  return res.json();
}

export async function deleteSession(id: string): Promise<void> {
  await fetch(`${BASE}/api/sessions/${id}`, { method: 'DELETE' });
}

export async function fetchModels(): Promise<ModelInfo[]> {
  const res = await fetch(`${BASE}/api/models`);
  const data = await res.json();
  return data.models ?? [];
}

export async function fetchProviders(): Promise<Provider[]> {
  const res = await fetch(`${BASE}/api/config/providers`);
  const data = await res.json();
  return data.providers ?? [];
}

export async function addProvider(provider: { name: string; api_base: string; api_key: string }): Promise<void> {
  await fetch(`${BASE}/api/config/providers`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(provider),
  });
}

export async function updateProvider(name: string, updates: { api_base?: string; api_key?: string }): Promise<void> {
  await fetch(`${BASE}/api/config/providers/${encodeURIComponent(name)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  });
}

export async function deleteProvider(name: string): Promise<void> {
  await fetch(`${BASE}/api/config/providers/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

export async function testProvider(name: string): Promise<{ ok: boolean; error?: string }> {
  const res = await fetch(`${BASE}/api/config/providers/${encodeURIComponent(name)}/test`, { method: 'POST' });
  return res.json();
}

export async function fetchMcpServers(): Promise<McpServer[]> {
  const res = await fetch(`${BASE}/api/config/mcp`);
  const data = await res.json();
  return data.mcp_servers ?? [];
}

export async function addMcpServer(server: { name: string; url: string; api_key?: string }): Promise<void> {
  await fetch(`${BASE}/api/config/mcp`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(server),
  });
}

export async function deleteMcpServer(name: string): Promise<void> {
  await fetch(`${BASE}/api/config/mcp/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

export async function fetchDaemonConfig(): Promise<Record<string, unknown>> {
  const res = await fetch(`${BASE}/api/config`);
  return res.json();
}

export async function restartDaemon(): Promise<{ restarting: boolean }> {
  const res = await fetch(`${BASE}/api/daemon/restart`, { method: 'POST' });
  return res.json();
}

export async function uploadFile(file: File): Promise<{
  file_id: string;
  url: string;
  filename: string;
  size: number;
  content_type: string;
}> {
  const form = new FormData();
  form.append('file', file);
  const res = await fetch(`${BASE}/api/upload`, { method: 'POST', body: form });
  return res.json();
}

export function createChatWebSocket(): WebSocket {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return new WebSocket(`${proto}//${window.location.host}/ws`);
}
