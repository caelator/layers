import { useEffect } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import {
  Plus,
  MessageSquare,
  LayoutDashboard,
  Settings,
  Trash2,
  X,
} from 'lucide-react';
import { useChatStore } from '../stores/chat';
import { LayersLogo } from './LayersLogo';

export function Sidebar() {
  const {
    sessions,
    activeSessionId,
    sidebarOpen,
    loadSessions,
    setActiveSession,
    newChat,
    removeSession,
    toggleSidebar,
  } = useChatStore();

  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  const handleNewChat = async () => {
    await newChat();
    navigate('/');
  };

  const handleSelectSession = (id: string) => {
    setActiveSession(id);
    navigate('/');
  };

  const formatDate = (dateStr: string) => {
    const d = new Date(dateStr);
    const now = new Date();
    const diff = now.getTime() - d.getTime();
    const days = Math.floor(diff / (1000 * 60 * 60 * 24));
    if (days === 0) return 'Today';
    if (days === 1) return 'Yesterday';
    if (days < 7) return `${days} days ago`;
    return d.toLocaleDateString();
  };

  return (
    <>
      {/* Mobile overlay */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 bg-black/50 z-40 lg:hidden"
          onClick={toggleSidebar}
        />
      )}

      <aside
        className={`fixed lg:relative z-50 h-full flex flex-col bg-bg-secondary border-r border-border transition-all duration-300 ${
          sidebarOpen ? 'w-72 translate-x-0' : 'w-0 -translate-x-72 lg:w-0'
        } overflow-hidden`}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-border shrink-0">
          <div className="flex items-center gap-2.5">
            <div className="text-accent">
              <LayersLogo size={26} />
            </div>
            <span className="font-semibold text-text-primary tracking-tight">Layers</span>
          </div>
          <button
            onClick={toggleSidebar}
            className="p-1.5 rounded-lg hover:bg-bg-hover transition-colors text-text-secondary lg:hidden"
          >
            <X size={18} />
          </button>
        </div>

        {/* New Chat */}
        <div className="p-3 shrink-0">
          <button
            onClick={handleNewChat}
            className="w-full flex items-center gap-2 px-3 py-2.5 rounded-lg border border-border-light hover:bg-bg-hover transition-all text-sm text-text-primary hover:border-accent/30"
          >
            <Plus size={16} />
            New Chat
          </button>
        </div>

        {/* Sessions List */}
        <div className="flex-1 overflow-y-auto px-2 pb-2">
          {sessions.length === 0 ? (
            <div className="px-3 py-8 text-center text-text-muted text-sm">
              No conversations yet
            </div>
          ) : (
            <div className="space-y-0.5">
              {sessions.map((session) => (
                <div
                  key={session.id}
                  className={`group flex items-center gap-2 px-3 py-2.5 rounded-lg cursor-pointer transition-colors text-sm ${
                    activeSessionId === session.id && location.pathname === '/'
                      ? 'bg-bg-active text-text-primary'
                      : 'text-text-secondary hover:bg-bg-hover hover:text-text-primary'
                  }`}
                  onClick={() => handleSelectSession(session.id)}
                >
                  <MessageSquare size={14} className="shrink-0 opacity-50" />
                  <div className="flex-1 min-w-0">
                    <div className="truncate">
                      {session.metadata?.title as string || `Chat ${session.id.slice(0, 8)}`}
                    </div>
                    <div className="text-xs text-text-muted mt-0.5">
                      {formatDate(session.updated_at)}
                    </div>
                  </div>
                  <button
                    onClick={(e) => { e.stopPropagation(); removeSession(session.id); }}
                    className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-bg-active transition-all text-text-muted hover:text-error"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Bottom Nav */}
        <div className="border-t border-border p-2 space-y-0.5 shrink-0">
          <button
            onClick={() => navigate('/dashboard')}
            className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-colors ${
              location.pathname === '/dashboard'
                ? 'bg-bg-active text-text-primary'
                : 'text-text-secondary hover:bg-bg-hover hover:text-text-primary'
            }`}
          >
            <LayoutDashboard size={16} />
            Dashboard
          </button>
          <button
            onClick={() => navigate('/settings')}
            className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-colors ${
              location.pathname === '/settings'
                ? 'bg-bg-active text-text-primary'
                : 'text-text-secondary hover:bg-bg-hover hover:text-text-primary'
            }`}
          >
            <Settings size={16} />
            Settings
          </button>
        </div>
      </aside>
    </>
  );
}
