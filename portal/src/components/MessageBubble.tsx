import { User, Bot, Image, FileText } from 'lucide-react';
import { MarkdownRenderer } from './MarkdownRenderer';
import type { ChatMessage } from '../types';

interface MessageBubbleProps {
  message: ChatMessage;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === 'user';

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div className={`flex gap-4 animate-fade-in ${isUser ? 'flex-row-reverse' : ''}`}>
      {/* Avatar */}
      <div
        className={`shrink-0 w-8 h-8 rounded-full flex items-center justify-center ${
          isUser ? 'bg-accent/20 text-accent' : 'bg-bg-tertiary text-text-secondary'
        }`}
      >
        {isUser ? <User size={16} /> : <Bot size={16} />}
      </div>

      {/* Content */}
      <div className={`flex-1 min-w-0 ${isUser ? 'text-right' : ''}`}>
        <div className="text-xs text-text-muted mb-1.5">
          {isUser ? 'You' : 'Layers'}
        </div>

        {/* Attachments */}
        {message.attachments && message.attachments.length > 0 && (
          <div className={`flex flex-wrap gap-2 mb-2 ${isUser ? 'justify-end' : ''}`}>
            {message.attachments.map((a) => (
              <div
                key={a.file_id}
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-bg-tertiary border border-border text-sm"
              >
                {a.type.startsWith('image/') ? (
                  a.preview_url ? (
                    <img src={a.preview_url} alt={a.name} className="w-16 h-16 rounded object-cover" />
                  ) : (
                    <Image size={14} className="text-accent" />
                  )
                ) : (
                  <FileText size={14} className="text-text-secondary" />
                )}
                <div className="text-left">
                  <div className="text-text-primary text-xs truncate max-w-[160px]">{a.name}</div>
                  <div className="text-text-muted text-xs">{formatSize(a.size)}</div>
                </div>
              </div>
            ))}
          </div>
        )}

        <div
          className={`inline-block text-left max-w-full ${
            isUser
              ? 'bg-accent/15 border border-accent/20 text-text-primary px-4 py-2.5 rounded-2xl rounded-tr-md'
              : ''
          }`}
        >
          {isUser ? (
            <p className="text-sm leading-relaxed whitespace-pre-wrap">{message.content}</p>
          ) : (
            <div className="text-sm">
              <MarkdownRenderer content={message.content} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
