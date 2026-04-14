import { useEffect, useRef } from 'react';
import { Menu, Layers } from 'lucide-react';
import { useChatStore } from '../stores/chat';
import { ModelSelector } from '../components/ModelSelector';
import { ChatInput } from '../components/ChatInput';
import { MessageBubble } from '../components/MessageBubble';
import { TypingIndicator } from '../components/TypingIndicator';
import { MarkdownRenderer } from '../components/MarkdownRenderer';

export function ChatPage() {
  const {
    activeSessionId,
    messages,
    isStreaming,
    streamingContent,
    connectWebSocket,
    disconnectWebSocket,
    toggleSidebar,
    newChat,
  } = useChatStore();

  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    connectWebSocket();
    return () => disconnectWebSocket();
  }, [connectWebSocket, disconnectWebSocket]);

  // Auto-scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, streamingContent, activeSessionId]);

  const currentMessages = activeSessionId ? (messages[activeSessionId] ?? []) : [];

  return (
    <div className="flex flex-col h-full">
      {/* Top bar */}
      <header className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <div className="flex items-center gap-3">
          <button
            onClick={toggleSidebar}
            className="p-1.5 rounded-lg hover:bg-bg-hover transition-colors text-text-secondary"
          >
            <Menu size={20} />
          </button>
          <ModelSelector />
        </div>
      </header>

      {/* Messages area */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        {currentMessages.length === 0 && !isStreaming ? (
          /* Empty state */
          <div className="flex flex-col items-center justify-center h-full gap-6 px-4">
            <div className="w-16 h-16 rounded-2xl bg-accent/10 flex items-center justify-center text-accent">
              <Layers size={32} />
            </div>
            <div className="text-center max-w-md">
              <h2 className="text-xl font-semibold text-text-primary mb-2">
                How can I help you today?
              </h2>
              <p className="text-text-secondary text-sm leading-relaxed">
                Start a conversation with Layers. Ask questions, share files,
                or explore your multi-model AI council.
              </p>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 max-w-lg w-full">
              {[
                'Explain how the routing layer works',
                'Compare outputs from different models',
                'Help me debug this error',
                'Summarize the current session history',
              ].map((prompt) => (
                <button
                  key={prompt}
                  onClick={() => {
                    if (!activeSessionId) newChat();
                    useChatStore.getState().sendMessage(prompt);
                  }}
                  className="text-left px-4 py-3 rounded-xl border border-border hover:border-border-light hover:bg-bg-secondary transition-all text-sm text-text-secondary hover:text-text-primary"
                >
                  {prompt}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="max-w-4xl mx-auto px-4 py-6 space-y-6">
            {currentMessages.map((msg) => (
              <MessageBubble key={msg.id} message={msg} />
            ))}

            {/* Streaming response */}
            {isStreaming && (
              <div className="flex gap-4 animate-fade-in">
                <div className="shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-bg-tertiary text-text-secondary">
                  <Layers size={16} />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-xs text-text-muted mb-1.5">Layers</div>
                  {streamingContent ? (
                    <div className="text-sm">
                      <MarkdownRenderer content={streamingContent} />
                      <span className="inline-block w-1.5 h-4 bg-accent/70 animate-pulse-soft ml-0.5 align-middle" />
                    </div>
                  ) : (
                    <TypingIndicator />
                  )}
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Input */}
      <ChatInput />
    </div>
  );
}
