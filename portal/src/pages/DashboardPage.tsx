import { useEffect, useState } from 'react';
import {
  Activity,
  Clock,
  MessageSquare,
  Wifi,
  WifiOff,
  RefreshCw,
  Menu,
} from 'lucide-react';
import { fetchHealth, fetchStatus, fetchSessions } from '../lib/api';
import { useChatStore } from '../stores/chat';
import type { HealthResponse, DaemonStatus, Session } from '../types';

export function DashboardPage() {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [status, setStatus] = useState<DaemonStatus | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const toggleSidebar = useChatStore((s) => s.toggleSidebar);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [h, s, sess] = await Promise.all([
        fetchHealth(),
        fetchStatus(),
        fetchSessions(),
      ]);
      setHealth(h);
      setStatus(s);
      setSessions(sess);
    } catch (e) {
      setError(`Unable to connect to daemon: ${e}`);
    }
    setLoading(false);
  };

  useEffect(() => { loadData(); }, []);

  const formatUptime = (secs: number) => {
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return `${h}h ${m}m`;
  };

  return (
    <div className="flex flex-col h-full">
      <header className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <div className="flex items-center gap-3">
          <button
            onClick={toggleSidebar}
            className="p-1.5 rounded-lg hover:bg-bg-hover transition-colors text-text-secondary"
          >
            <Menu size={20} />
          </button>
          <h1 className="text-lg font-semibold text-text-primary">Dashboard</h1>
        </div>
        <button
          onClick={loadData}
          disabled={loading}
          className="p-2 rounded-lg hover:bg-bg-hover transition-colors text-text-secondary hover:text-text-primary disabled:opacity-50"
        >
          <RefreshCw size={18} className={loading ? 'animate-spin' : ''} />
        </button>
      </header>

      <div className="flex-1 overflow-y-auto p-6">
        {error && (
          <div className="mb-6 p-4 rounded-xl bg-error/10 border border-error/20 text-error text-sm">
            {error}
          </div>
        )}

        <div className="max-w-5xl mx-auto space-y-6">
          {/* Status cards */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            {/* Health */}
            <div className="p-5 rounded-xl bg-bg-secondary border border-border">
              <div className="flex items-center justify-between mb-3">
                <span className="text-text-secondary text-sm">Status</span>
                <Activity size={18} className={health?.status === 'ok' ? 'text-success' : 'text-error'} />
              </div>
              <div className="text-2xl font-semibold text-text-primary">
                {loading ? '...' : health?.status === 'ok' ? 'Online' : 'Offline'}
              </div>
              <div className="text-xs text-text-muted mt-1">
                v{health?.version ?? '—'}
              </div>
            </div>

            {/* Uptime */}
            <div className="p-5 rounded-xl bg-bg-secondary border border-border">
              <div className="flex items-center justify-between mb-3">
                <span className="text-text-secondary text-sm">Uptime</span>
                <Clock size={18} className="text-accent" />
              </div>
              <div className="text-2xl font-semibold text-text-primary">
                {loading ? '...' : formatUptime(status?.uptime_secs ?? 0)}
              </div>
            </div>

            {/* Sessions */}
            <div className="p-5 rounded-xl bg-bg-secondary border border-border">
              <div className="flex items-center justify-between mb-3">
                <span className="text-text-secondary text-sm">Sessions</span>
                <MessageSquare size={18} className="text-accent" />
              </div>
              <div className="text-2xl font-semibold text-text-primary">
                {loading ? '...' : sessions.length}
              </div>
              <div className="text-xs text-text-muted mt-1">
                {sessions.reduce((a, s) => a + s.message_count, 0)} messages total
              </div>
            </div>

            {/* Channels */}
            <div className="p-5 rounded-xl bg-bg-secondary border border-border">
              <div className="flex items-center justify-between mb-3">
                <span className="text-text-secondary text-sm">Channels</span>
                <Wifi size={18} className="text-accent" />
              </div>
              <div className="text-2xl font-semibold text-text-primary">
                {loading ? '...' : status?.channels.length ?? 0}
              </div>
              <div className="text-xs text-text-muted mt-1">active adapters</div>
            </div>
          </div>

          {/* Channel details */}
          {status && status.channels.length > 0 && (
            <div className="p-5 rounded-xl bg-bg-secondary border border-border">
              <h2 className="text-sm font-semibold text-text-primary mb-4">Channel Status</h2>
              <div className="space-y-2">
                {status.channels.map((ch) => (
                  <div
                    key={ch.name}
                    className="flex items-center justify-between px-4 py-3 rounded-lg bg-bg-primary"
                  >
                    <div className="flex items-center gap-3">
                      {ch.health === 'Connected' || ch.health === 'Healthy' ? (
                        <Wifi size={14} className="text-success" />
                      ) : (
                        <WifiOff size={14} className="text-error" />
                      )}
                      <span className="text-sm text-text-primary">{ch.name}</span>
                    </div>
                    <span
                      className={`text-xs px-2 py-1 rounded-full ${
                        ch.health === 'Connected' || ch.health === 'Healthy'
                          ? 'bg-success/10 text-success'
                          : 'bg-error/10 text-error'
                      }`}
                    >
                      {ch.health}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Recent sessions */}
          {sessions.length > 0 && (
            <div className="p-5 rounded-xl bg-bg-secondary border border-border">
              <h2 className="text-sm font-semibold text-text-primary mb-4">Recent Sessions</h2>
              <div className="space-y-2">
                {sessions.slice(0, 10).map((s) => (
                  <div
                    key={s.id}
                    className="flex items-center justify-between px-4 py-3 rounded-lg bg-bg-primary"
                  >
                    <div>
                      <div className="text-sm text-text-primary">
                        {(s.metadata?.title as string) || s.id.slice(0, 12)}
                      </div>
                      <div className="text-xs text-text-muted mt-0.5">
                        {s.message_count} messages &middot; {s.token_count} tokens
                      </div>
                    </div>
                    <div className="text-xs text-text-muted">
                      {new Date(s.updated_at).toLocaleDateString()}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
