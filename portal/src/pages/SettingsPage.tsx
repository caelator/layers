import { useState, useEffect, useCallback } from 'react';
import {
  Menu, Moon, Sun, Save, Check, Plus, Trash2, Server, Key, Cpu,
  Settings2, Globe, RotateCcw, Download, Upload, ChevronDown,
  ChevronRight, Eye, EyeOff, Zap, X, Monitor, Database
} from 'lucide-react';
import { useSettingsStore } from '../stores/settings';
import { useChatStore } from '../stores/chat';
import {
  fetchModels, fetchProviders, addProvider,
  deleteProvider, testProvider, fetchMcpServers, addMcpServer,
  deleteMcpServer, restartDaemon
} from '../lib/api';
import type { ModelInfo, Provider, McpServer } from '../types';

// Toast notification component
function Toast({ message, type, onClose }: { message: string; type: 'success' | 'error'; onClose: () => void }) {
  useEffect(() => { const t = setTimeout(onClose, 3000); return () => clearTimeout(t); }, [onClose]);
  return (
    <div className={`fixed bottom-4 right-4 px-4 py-3 rounded-lg shadow-lg flex items-center gap-2 z-50 animate-slide-up ${
      type === 'success' ? 'bg-emerald-600/90 text-white' : 'bg-red-600/90 text-white'
    }`}>
      {type === 'success' ? <Check size={16} /> : <X size={16} />}
      <span className="text-sm">{message}</span>
    </div>
  );
}

// Collapsible section
function Section({ title, icon: Icon, children, defaultOpen = false }: {
  title: string; icon: React.ComponentType<{ size?: number; className?: string }>;
  children: React.ReactNode; defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="rounded-xl bg-bg-secondary border border-border overflow-hidden">
      <button onClick={() => setOpen(!open)} className="w-full flex items-center gap-3 px-5 py-4 hover:bg-bg-hover transition-colors">
        <Icon size={18} className="text-accent" />
        <span className="text-sm font-semibold text-text-primary flex-1 text-left">{title}</span>
        {open ? <ChevronDown size={16} className="text-text-muted" /> : <ChevronRight size={16} className="text-text-muted" />}
      </button>
      {open && <div className="px-5 pb-5 border-t border-border">{children}</div>}
    </div>
  );
}

// Status dot
function StatusDot({ status }: { status: string }) {
  const color = status === 'connected' ? 'bg-emerald-500' : status === 'error' ? 'bg-red-500' : 'bg-yellow-500';
  return <span className={`inline-block w-2 h-2 rounded-full ${color}`} title={status} />;
}

// Slider component
function Slider({ label, value, min, max, step, onChange }: {
  label: string; value: number; min: number; max: number; step: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="space-y-1">
      <div className="flex justify-between text-xs">
        <span className="text-text-secondary">{label}</span>
        <span className="text-text-muted font-mono">{value}</span>
      </div>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(parseFloat(e.target.value))}
        className="w-full h-1.5 rounded-full appearance-none bg-bg-tertiary accent-accent" />
    </div>
  );
}

export function SettingsPage() {
  const {
    theme, selectedModel, apiEndpoint, modelConfig, systemPrompt,
    presets, activePreset, workspacePath, timezone, logLevel,
    toggleTheme, setModel, setApiEndpoint, setModelConfig,
    setSystemPrompt, savePreset, loadPreset, deletePreset,
    setWorkspacePath, setTimezone, setLogLevel
  } = useSettingsStore();
  const toggleSidebar = useChatStore((s) => s.toggleSidebar);

  const [models, setModels] = useState<ModelInfo[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [localEndpoint, setLocalEndpoint] = useState(apiEndpoint);
  const [saved, setSaved] = useState(false);
  const [toast, setToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  // New provider form
  const [newProvider, setNewProvider] = useState({ name: '', api_base: '', api_key: '' });
  const [showNewProvider, setShowNewProvider] = useState(false);
  const [showKeyMap, setShowKeyMap] = useState<Record<string, boolean>>({});


  // New MCP server form
  const [newMcp, setNewMcp] = useState({ name: '', url: '', api_key: '' });
  const [showNewMcp, setShowNewMcp] = useState(false);

  // Preset name input
  const [presetName, setPresetName] = useState('');

  // Delete confirmation
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const showToast = useCallback((message: string, type: 'success' | 'error' = 'success') => {
    setToast({ message, type });
  }, []);

  useEffect(() => {
    fetchModels().then(setModels).catch(() => setModels([{ id: 'default', name: 'Default Model', provider: 'layers' }]));
    fetchProviders().then(setProviders).catch(() => {});
    fetchMcpServers().then(setMcpServers).catch(() => {});
  }, []);

  const handleSaveEndpoint = () => { setApiEndpoint(localEndpoint); setSaved(true); setTimeout(() => setSaved(false), 2000); };

  const handleAddProvider = async () => {
    try {
      await addProvider(newProvider);
      setProviders((prev) => [...prev, { ...newProvider, api_key_set: true, models: [], status: 'untested' as const }]);
      setNewProvider({ name: '', api_base: '', api_key: '' });
      setShowNewProvider(false);
      showToast('Provider added');
    } catch { showToast('Failed to add provider', 'error'); }
  };

  const handleDeleteProvider = async (name: string) => {
    try {
      await deleteProvider(name);
      setProviders((prev) => prev.filter((p) => p.name !== name));
      setConfirmDelete(null);
      showToast('Provider deleted');
    } catch { showToast('Failed to delete provider', 'error'); }
  };

  const handleTestProvider = async (name: string) => {
    try {
      const result = await testProvider(name);
      setProviders((prev) => prev.map((p) => p.name === name ? { ...p, status: result.ok ? 'connected' as const : 'error' as const } : p));
      showToast(result.ok ? 'Connection OK' : result.error ?? 'Connection failed', result.ok ? 'success' : 'error');
    } catch { showToast('Test failed', 'error'); }
  };

  const handleAddMcp = async () => {
    try {
      await addMcpServer(newMcp);
      setMcpServers((prev) => [...prev, { ...newMcp, api_key_set: !!newMcp.api_key, tools: [], status: 'untested' as const }]);
      setNewMcp({ name: '', url: '', api_key: '' });
      setShowNewMcp(false);
      showToast('MCP server added');
    } catch { showToast('Failed to add MCP server', 'error'); }
  };

  const handleDeleteMcp = async (name: string) => {
    try {
      await deleteMcpServer(name);
      setMcpServers((prev) => prev.filter((s) => s.name !== name));
      setConfirmDelete(null);
      showToast('MCP server deleted');
    } catch { showToast('Failed to delete MCP server', 'error'); }
  };

  const handleExport = () => {
    const data = JSON.stringify({ theme, selectedModel, modelConfig, systemPrompt, presets, workspacePath, timezone, logLevel }, null, 2);
    const blob = new Blob([data], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a'); a.href = url; a.download = 'layers-settings.json'; a.click();
    URL.revokeObjectURL(url);
    showToast('Settings exported');
  };

  const handleImport = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      try {
        const data = JSON.parse(ev.target?.result as string);
        if (data.theme) toggleTheme(); // just trigger re-render
        if (data.selectedModel) setModel(data.selectedModel);
        if (data.modelConfig) setModelConfig(data.modelConfig);
        if (data.systemPrompt !== undefined) setSystemPrompt(data.systemPrompt);
        if (data.workspacePath) setWorkspacePath(data.workspacePath);
        if (data.timezone) setTimezone(data.timezone);
        if (data.logLevel) setLogLevel(data.logLevel);
        showToast('Settings imported');
      } catch { showToast('Invalid settings file', 'error'); }
    };
    reader.readAsText(file);
  };

  const handleRestart = async () => {
    try {
      await restartDaemon();
      showToast('Daemon restarting...');
    } catch { showToast('Restart failed', 'error'); }
  };

  return (
    <div className="flex flex-col h-full">
      {toast && <Toast message={toast.message} type={toast.type} onClose={() => setToast(null)} />}

      <header className="flex items-center gap-3 px-4 py-3 border-b border-border shrink-0">
        <button onClick={toggleSidebar} className="p-1.5 rounded-lg hover:bg-bg-hover transition-colors text-text-secondary">
          <Menu size={20} />
        </button>
        <h1 className="text-lg font-semibold text-text-primary">Settings</h1>
      </header>

      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-2xl mx-auto space-y-4">

          {/* Appearance */}
          <Section title="Appearance" icon={Monitor} defaultOpen>
            <div className="flex items-center justify-between py-2">
              <div><div className="text-sm text-text-primary">Theme</div>
                <div className="text-xs text-text-muted mt-0.5">Switch between dark and light mode</div></div>
              <button onClick={toggleTheme} className="flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary hover:bg-bg-hover transition-colors text-sm">
                {theme === 'dark' ? <><Moon size={14} className="text-accent" /><span className="text-text-primary">Dark</span></>
                  : <><Sun size={14} className="text-yellow-400" /><span className="text-text-primary">Light</span></>}
              </button>
            </div>
          </Section>

          {/* Connection */}
          <Section title="Connection" icon={Globe} defaultOpen>
            <label className="block text-sm text-text-secondary mb-2">Daemon Endpoint</label>
            <div className="flex gap-2">
              <input type="text" value={localEndpoint} onChange={(e) => setLocalEndpoint(e.target.value)}
                className="flex-1 px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50 transition-colors"
                placeholder="http://127.0.0.1:18791" />
              <button onClick={handleSaveEndpoint} className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover transition-colors text-white text-sm">
                {saved ? <Check size={14} /> : <Save size={14} />}{saved ? 'Saved' : 'Save'}
              </button>
            </div>
            <div className="mt-3 space-y-2">
              <label className="block text-sm text-text-secondary">Workspace Path</label>
              <input type="text" value={workspacePath} onChange={(e) => setWorkspacePath(e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50 transition-colors"
                placeholder="/path/to/workspace" />
            </div>
            <div className="mt-3 grid grid-cols-2 gap-3">
              <div>
                <label className="block text-sm text-text-secondary mb-1">Timezone</label>
                <select value={timezone} onChange={(e) => setTimezone(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50">
                  {['America/Bogota','America/New_York','America/Chicago','America/Denver','America/Los_Angeles','Europe/London','Europe/Berlin','Asia/Tokyo','UTC'].map((tz) => (
                    <option key={tz} value={tz}>{tz}</option>
                  ))}
                </select>
              </div>
              <div>
                <label className="block text-sm text-text-secondary mb-1">Log Level</label>
                <select value={logLevel} onChange={(e) => setLogLevel(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50">
                  {['trace','debug','info','warn','error'].map((l) => (
                    <option key={l} value={l}>{l}</option>
                  ))}
                </select>
              </div>
            </div>
          </Section>

          {/* Model Configuration */}
          <Section title="Model Configuration" icon={Cpu} defaultOpen>
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-text-secondary mb-2">Model</label>
                <div className="space-y-1">
                  {models.map((m) => (
                    <label key={m.id} className={`flex items-center justify-between px-4 py-2.5 rounded-lg cursor-pointer transition-colors ${
                      selectedModel === m.id ? 'bg-accent-soft border border-accent/30' : 'bg-bg-primary hover:bg-bg-hover border border-transparent'
                    }`}>
                      <div className="flex items-center gap-3">
                        <input type="radio" name="model" value={m.id} checked={selectedModel === m.id} onChange={() => setModel(m.id)} className="accent-accent" />
                        <div><div className="text-sm text-text-primary">{m.name}</div><div className="text-xs text-text-muted">{m.provider}</div></div>
                      </div>
                      {selectedModel === m.id && <div className="w-2 h-2 rounded-full bg-accent" />}
                    </label>
                  ))}
                </div>
              </div>

              <div className="space-y-3 pt-2 border-t border-border">
                <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider">Parameters</h3>
                <Slider label="Temperature" value={modelConfig.temperature} min={0} max={2} step={0.1} onChange={(v) => setModelConfig({ temperature: v })} />
                <Slider label="Top P" value={modelConfig.top_p} min={0} max={1} step={0.05} onChange={(v) => setModelConfig({ top_p: v })} />
                <Slider label="Frequency Penalty" value={modelConfig.frequency_penalty} min={-2} max={2} step={0.1} onChange={(v) => setModelConfig({ frequency_penalty: v })} />
                <Slider label="Presence Penalty" value={modelConfig.presence_penalty} min={-2} max={2} step={0.1} onChange={(v) => setModelConfig({ presence_penalty: v })} />

                <div className="flex gap-3">
                  <div className="flex-1">
                    <label className="block text-xs text-text-secondary mb-1">Max Tokens</label>
                    <input type="number" value={modelConfig.max_tokens} onChange={(e) => setModelConfig({ max_tokens: parseInt(e.target.value) || 4096 })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50" />
                  </div>
                  <div className="flex-1">
                    <label className="block text-xs text-text-secondary mb-1">Context Window</label>
                    <input type="number" value={modelConfig.context_window} onChange={(e) => setModelConfig({ context_window: parseInt(e.target.value) || 128000 })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50" />
                  </div>
                </div>

                <div>
                  <label className="block text-xs text-text-secondary mb-1">Thinking Mode</label>
                  <select value={modelConfig.thinking} onChange={(e) => setModelConfig({ thinking: e.target.value as 'off' | 'on' | 'stream' })}
                    className="w-full px-3 py-1.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50">
                    <option value="off">Off</option>
                    <option value="on">On</option>
                    <option value="stream">Stream</option>
                  </select>
                </div>
              </div>

              <div className="pt-2 border-t border-border">
                <label className="block text-xs text-text-secondary mb-1">System Prompt</label>
                <textarea value={systemPrompt} onChange={(e) => setSystemPrompt(e.target.value)} rows={4}
                  className="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50 resize-y"
                  placeholder="Enter system prompt..." />
              </div>

              {/* Presets */}
              <div className="pt-2 border-t border-border">
                <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-2">Presets</h3>
                {presets.length > 0 && (
                  <div className="space-y-1 mb-3">
                    {presets.map((p) => (
                      <div key={p.id} className={`flex items-center justify-between px-3 py-2 rounded-lg ${
                        activePreset === p.id ? 'bg-accent-soft border border-accent/30' : 'bg-bg-primary border border-transparent'
                      }`}>
                        <button onClick={() => loadPreset(p.id)} className="text-sm text-text-primary hover:text-accent transition-colors text-left flex-1">
                          {p.name} <span className="text-text-muted">({p.model_id})</span>
                        </button>
                        <button onClick={() => deletePreset(p.id)} className="p-1 hover:text-red-400 text-text-muted transition-colors">
                          <Trash2 size={14} />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
                <div className="flex gap-2">
                  <input type="text" value={presetName} onChange={(e) => setPresetName(e.target.value)}
                    className="flex-1 px-3 py-1.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50"
                    placeholder="Preset name..." />
                  <button disabled={!presetName.trim()} onClick={() => { savePreset(presetName); setPresetName(''); showToast('Preset saved'); }}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed text-white text-sm transition-colors">
                    <Save size={14} /> Save
                  </button>
                </div>
              </div>
            </div>
          </Section>

          {/* Providers / API Keys */}
          <Section title="Providers & API Keys" icon={Key}>
            <div className="space-y-3">
              {providers.map((p) => (
                <div key={p.name} className="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-primary border border-border">
                  <StatusDot status={p.status} />
                  <div className="flex-1 min-w-0">
                    <div className="text-sm text-text-primary font-medium">{p.name}</div>
                    <div className="text-xs text-text-muted truncate">{p.api_base}</div>
                  </div>
                  <button onClick={() => handleTestProvider(p.name)} className="p-1.5 rounded-lg hover:bg-bg-hover text-text-muted hover:text-accent transition-colors" title="Test connection">
                    <Zap size={14} />
                  </button>
                  {confirmDelete === `provider-${p.name}` ? (
                    <div className="flex gap-1">
                      <button onClick={() => handleDeleteProvider(p.name)} className="px-2 py-1 text-xs bg-red-600 text-white rounded-lg hover:bg-red-700">Delete</button>
                      <button onClick={() => setConfirmDelete(null)} className="px-2 py-1 text-xs bg-bg-tertiary text-text-primary rounded-lg hover:bg-bg-hover">Cancel</button>
                    </div>
                  ) : (
                    <button onClick={() => setConfirmDelete(`provider-${p.name}`)} className="p-1.5 rounded-lg hover:bg-bg-hover text-text-muted hover:text-red-400 transition-colors">
                      <Trash2 size={14} />
                    </button>
                  )}
                </div>
              ))}

              {showNewProvider ? (
                <div className="p-4 rounded-lg bg-bg-primary border border-accent/30 space-y-3">
                  <div>
                    <label className="block text-xs text-text-secondary mb-1">Provider Name</label>
                    <input type="text" value={newProvider.name} onChange={(e) => setNewProvider({ ...newProvider, name: e.target.value })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-tertiary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50"
                      placeholder="e.g. openai, anthropic, zai" />
                  </div>
                  <div>
                    <label className="block text-xs text-text-secondary mb-1">API Base URL</label>
                    <input type="text" value={newProvider.api_base} onChange={(e) => setNewProvider({ ...newProvider, api_base: e.target.value })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-tertiary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50"
                      placeholder="https://api.openai.com/v1" />
                  </div>
                  <div>
                    <label className="block text-xs text-text-secondary mb-1">API Key</label>
                    <div className="relative">
                      <input type={showKeyMap['new'] ? 'text' : 'password'} value={newProvider.api_key}
                        onChange={(e) => setNewProvider({ ...newProvider, api_key: e.target.value })}
                        className="w-full px-3 py-1.5 pr-10 rounded-lg bg-bg-tertiary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50"
                        placeholder="sk-..." />
                      <button onClick={() => setShowKeyMap({ ...showKeyMap, new: !showKeyMap['new'] })}
                        className="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-primary">
                        {showKeyMap['new'] ? <EyeOff size={14} /> : <Eye size={14} />}
                      </button>
                    </div>
                  </div>
                  <div className="flex gap-2 justify-end">
                    <button onClick={() => setShowNewProvider(false)} className="px-3 py-1.5 text-sm rounded-lg bg-bg-tertiary hover:bg-bg-hover text-text-primary transition-colors">Cancel</button>
                    <button onClick={handleAddProvider} disabled={!newProvider.name || !newProvider.api_key}
                      className="px-3 py-1.5 text-sm rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-40 text-white transition-colors">Add Provider</button>
                  </div>
                </div>
              ) : (
                <button onClick={() => setShowNewProvider(true)}
                  className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg border border-dashed border-border hover:border-accent/50 hover:bg-bg-hover transition-colors text-sm text-text-muted hover:text-accent">
                  <Plus size={16} /> Add Provider
                </button>
              )}
            </div>
          </Section>

          {/* MCP Servers */}
          <Section title="MCP Servers" icon={Server}>
            <div className="space-y-3">
              {mcpServers.map((s) => (
                <div key={s.name} className="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-primary border border-border">
                  <StatusDot status={s.status} />
                  <div className="flex-1 min-w-0">
                    <div className="text-sm text-text-primary font-medium">{s.name}</div>
                    <div className="text-xs text-text-muted truncate">{s.url}</div>
                  </div>
                  {confirmDelete === `mcp-${s.name}` ? (
                    <div className="flex gap-1">
                      <button onClick={() => handleDeleteMcp(s.name)} className="px-2 py-1 text-xs bg-red-600 text-white rounded-lg hover:bg-red-700">Delete</button>
                      <button onClick={() => setConfirmDelete(null)} className="px-2 py-1 text-xs bg-bg-tertiary text-text-primary rounded-lg hover:bg-bg-hover">Cancel</button>
                    </div>
                  ) : (
                    <button onClick={() => setConfirmDelete(`mcp-${s.name}`)} className="p-1.5 rounded-lg hover:bg-bg-hover text-text-muted hover:text-red-400 transition-colors">
                      <Trash2 size={14} />
                    </button>
                  )}
                </div>
              ))}

              {showNewMcp ? (
                <div className="p-4 rounded-lg bg-bg-primary border border-accent/30 space-y-3">
                  <div>
                    <label className="block text-xs text-text-secondary mb-1">Server Name</label>
                    <input type="text" value={newMcp.name} onChange={(e) => setNewMcp({ ...newMcp, name: e.target.value })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-tertiary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50" placeholder="e.g. gitnexus" />
                  </div>
                  <div>
                    <label className="block text-xs text-text-secondary mb-1">URL</label>
                    <input type="text" value={newMcp.url} onChange={(e) => setNewMcp({ ...newMcp, url: e.target.value })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-tertiary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50" placeholder="http://127.0.0.1:8080" />
                  </div>
                  <div>
                    <label className="block text-xs text-text-secondary mb-1">API Key (optional)</label>
                    <input type="password" value={newMcp.api_key} onChange={(e) => setNewMcp({ ...newMcp, api_key: e.target.value })}
                      className="w-full px-3 py-1.5 rounded-lg bg-bg-tertiary border border-border text-sm text-text-primary focus:outline-none focus:border-accent/50" />
                  </div>
                  <div className="flex gap-2 justify-end">
                    <button onClick={() => setShowNewMcp(false)} className="px-3 py-1.5 text-sm rounded-lg bg-bg-tertiary hover:bg-bg-hover text-text-primary transition-colors">Cancel</button>
                    <button onClick={handleAddMcp} disabled={!newMcp.name || !newMcp.url}
                      className="px-3 py-1.5 text-sm rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-40 text-white transition-colors">Add Server</button>
                  </div>
                </div>
              ) : (
                <button onClick={() => setShowNewMcp(true)}
                  className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg border border-dashed border-border hover:border-accent/50 hover:bg-bg-hover transition-colors text-sm text-text-muted hover:text-accent">
                  <Plus size={16} /> Add MCP Server
                </button>
              )}
            </div>
          </Section>

          {/* Daemon */}
          <Section title="Daemon" icon={Database}>
            <div className="space-y-3">
              <div className="flex items-center justify-between py-2">
                <div><div className="text-sm text-text-primary">Restart Daemon</div>
                  <div className="text-xs text-text-muted">Restarts the Layers daemon process</div></div>
                <button onClick={handleRestart}
                  className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-bg-tertiary hover:bg-bg-hover transition-colors text-sm text-text-primary">
                  <RotateCcw size={14} /> Restart
                </button>
              </div>
            </div>
          </Section>

          {/* Export/Import */}
          <Section title="Export / Import" icon={Settings2}>
            <div className="flex gap-3">
              <button onClick={handleExport}
                className="flex-1 flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary hover:bg-bg-hover transition-colors text-sm text-text-primary">
                <Download size={14} /> Export Settings
              </button>
              <label className="flex-1 flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary hover:bg-bg-hover transition-colors text-sm text-text-primary cursor-pointer">
                <Upload size={14} /> Import Settings
                <input type="file" accept=".json" onChange={handleImport} className="hidden" />
              </label>
            </div>
          </Section>

        </div>
      </div>
    </div>
  );
}
