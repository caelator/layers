import { create } from 'zustand';
import type { ChatMessage, Session, Attachment } from '../types';
import { fetchSessions, createSession, deleteSession, createChatWebSocket } from '../lib/api';

interface ChatState {
  sessions: Session[];
  activeSessionId: string | null;
  messages: Record<string, ChatMessage[]>;
  isStreaming: boolean;
  streamingContent: string;
  ws: WebSocket | null;
  sidebarOpen: boolean;
  pendingAttachments: Attachment[];

  loadSessions: () => Promise<void>;
  setActiveSession: (id: string) => void;
  newChat: () => Promise<void>;
  removeSession: (id: string) => Promise<void>;
  sendMessage: (text: string) => void;
  connectWebSocket: () => void;
  disconnectWebSocket: () => void;
  toggleSidebar: () => void;
  setSidebarOpen: (open: boolean) => void;
  addAttachment: (a: Attachment) => void;
  removeAttachment: (fileId: string) => void;
  clearAttachments: () => void;
}

export const useChatStore = create<ChatState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  messages: {},
  isStreaming: false,
  streamingContent: '',
  ws: null,
  sidebarOpen: true,
  pendingAttachments: [],

  loadSessions: async () => {
    try {
      const sessions = await fetchSessions();
      set({ sessions });
      if (sessions.length > 0 && !get().activeSessionId) {
        set({ activeSessionId: sessions[0].id });
      }
    } catch {
      // daemon may not be running
    }
  },

  setActiveSession: (id) => {
    set({ activeSessionId: id, streamingContent: '', isStreaming: false });
  },

  newChat: async () => {
    try {
      const session = await createSession();
      set((s) => ({
        sessions: [session, ...s.sessions],
        activeSessionId: session.id,
        streamingContent: '',
        isStreaming: false,
      }));
    } catch {
      // Create a local-only session for now
      const id = crypto.randomUUID();
      const session: Session = {
        id,
        agent_id: 'default',
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        model: null,
        metadata: {},
        message_count: 0,
        token_count: 0,
      };
      set((s) => ({
        sessions: [session, ...s.sessions],
        activeSessionId: id,
        messages: { ...s.messages, [id]: [] },
        streamingContent: '',
        isStreaming: false,
      }));
    }
  },

  removeSession: async (id) => {
    try {
      await deleteSession(id);
    } catch { /* ignore */ }
    set((s) => {
      const sessions = s.sessions.filter((ss) => ss.id !== id);
      const messages = { ...s.messages };
      delete messages[id];
      return {
        sessions,
        messages,
        activeSessionId: s.activeSessionId === id
          ? (sessions[0]?.id ?? null)
          : s.activeSessionId,
      };
    });
  },

  sendMessage: (text) => {
    const { activeSessionId, ws, pendingAttachments } = get();
    if (!activeSessionId) return;

    const userMsg: ChatMessage = {
      id: crypto.randomUUID(),
      role: 'user',
      content: text,
      timestamp: new Date().toISOString(),
      attachments: pendingAttachments.length > 0 ? [...pendingAttachments] : undefined,
    };

    set((s) => ({
      messages: {
        ...s.messages,
        [activeSessionId]: [...(s.messages[activeSessionId] ?? []), userMsg],
      },
      isStreaming: true,
      streamingContent: '',
      pendingAttachments: [],
    }));

    // Send via WebSocket
    if (ws && ws.readyState === WebSocket.OPEN) {
      // Get selected model from settings store
      const settingsStr = localStorage.getItem('layers-settings');
      const settings = settingsStr ? JSON.parse(settingsStr) : {};
      const model = settings?.state?.selectedModel || 'default';
      ws.send(JSON.stringify({
        message: text,
        model,
        session_id: activeSessionId,
        attachments: pendingAttachments.length > 0 ? pendingAttachments : undefined,
      }));
    }
  },

  connectWebSocket: () => {
    const existing = get().ws;
    if (existing && existing.readyState === WebSocket.OPEN) return;

    try {
      const ws = createChatWebSocket();

      ws.onmessage = (event) => {
        const data = event.data;
        if (typeof data === 'string') {
          try {
            const parsed = JSON.parse(data);
            if (parsed.type === 'done' || parsed.done) {
              // Streaming finished — commit message
              const { activeSessionId, streamingContent } = get();
              if (activeSessionId && streamingContent) {
                const assistantMsg: ChatMessage = {
                  id: crypto.randomUUID(),
                  role: 'assistant',
                  content: streamingContent,
                  timestamp: new Date().toISOString(),
                };
                set((s) => ({
                  messages: {
                    ...s.messages,
                    [activeSessionId]: [...(s.messages[activeSessionId] ?? []), assistantMsg],
                  },
                  isStreaming: false,
                  streamingContent: '',
                }));
              } else {
                set({ isStreaming: false, streamingContent: '' });
              }
              return;
            }
            if (parsed.text || parsed.content || parsed.chunk) {
              const chunk = parsed.content || parsed.text || parsed.chunk;
              set((s) => ({ streamingContent: s.streamingContent + chunk }));
              return;
            }
          } catch {
            // Not JSON — treat as raw text chunk
          }
          // Raw text streaming
          set((s) => ({ streamingContent: s.streamingContent + data }));
        }
      };

      ws.onclose = () => {
        set({ ws: null, isStreaming: false });
        // Auto-reconnect after 3s
        setTimeout(() => get().connectWebSocket(), 3000);
      };

      ws.onerror = () => {
        ws.close();
      };

      set({ ws });
    } catch {
      // WebSocket not available
    }
  },

  disconnectWebSocket: () => {
    const { ws } = get();
    if (ws) {
      ws.close();
      set({ ws: null });
    }
  },

  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  setSidebarOpen: (open) => set({ sidebarOpen: open }),

  addAttachment: (a) => set((s) => ({ pendingAttachments: [...s.pendingAttachments, a] })),
  removeAttachment: (fileId) => set((s) => ({
    pendingAttachments: s.pendingAttachments.filter((a) => a.file_id !== fileId),
  })),
  clearAttachments: () => set({ pendingAttachments: [] }),
}));
