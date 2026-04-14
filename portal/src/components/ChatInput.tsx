import { useState, useRef, useCallback } from 'react';
import { Send, Paperclip, X, Image, FileText } from 'lucide-react';
import { useChatStore } from '../stores/chat';
import { uploadFile } from '../lib/api';

export function ChatInput() {
  const [text, setText] = useState('');
  const [isDragOver, setIsDragOver] = useState(false);
  const [uploading, setUploading] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const {
    sendMessage,
    isStreaming,
    pendingAttachments,
    addAttachment,
    removeAttachment,
  } = useChatStore();

  const handleSend = () => {
    const trimmed = text.trim();
    if (!trimmed && pendingAttachments.length === 0) return;
    if (isStreaming) return;
    sendMessage(trimmed);
    setText('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setText(e.target.value);
    const ta = e.target;
    ta.style.height = 'auto';
    ta.style.height = Math.min(ta.scrollHeight, 200) + 'px';
  };

  const handleFiles = useCallback(async (files: FileList) => {
    setUploading(true);
    for (const file of Array.from(files)) {
      try {
        const result = await uploadFile(file);
        addAttachment({
          file_id: result.file_id,
          name: file.name,
          size: file.size,
          type: file.type,
          url: result.url,
          preview_url: (result as any).preview_url,
        });
      } catch {
        // Fallback: create local attachment reference
        addAttachment({
          file_id: crypto.randomUUID(),
          name: file.name,
          size: file.size,
          type: file.type,
        });
      }
    }
    setUploading(false);
  }, [addAttachment]);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);
    if (e.dataTransfer.files.length > 0) {
      handleFiles(e.dataTransfer.files);
    }
  }, [handleFiles]);

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  const isImage = (type: string) => type.startsWith('image/');

  return (
    <div className="px-4 pb-4 pt-2 max-w-4xl mx-auto w-full">
      {/* Attachments preview */}
      {pendingAttachments.length > 0 && (
        <div className="flex flex-wrap gap-2 mb-2 px-1">
          {pendingAttachments.map((a) => (
            <div
              key={a.file_id}
              className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-bg-tertiary border border-border text-sm animate-fade-in"
            >
              {isImage(a.type) ? (
                a.preview_url ? (
                  <img src={a.preview_url} alt="" className="w-8 h-8 rounded object-cover" />
                ) : (
                  <Image size={14} className="text-accent" />
                )
              ) : (
                <FileText size={14} className="text-text-secondary" />
              )}
              <span className="text-text-primary max-w-[120px] truncate">{a.name}</span>
              <span className="text-text-muted text-xs">{formatSize(a.size)}</span>
              <button
                onClick={() => removeAttachment(a.file_id)}
                className="p-0.5 rounded hover:bg-bg-hover text-text-muted hover:text-text-primary"
              >
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Input area */}
      <div
        className={`flex items-end gap-2 px-4 py-3 rounded-2xl border transition-all ${
          isDragOver
            ? 'border-accent bg-accent-soft'
            : 'border-border-light bg-bg-secondary hover:border-text-muted focus-within:border-accent/50'
        }`}
        onDragOver={(e) => { e.preventDefault(); setIsDragOver(true); }}
        onDragLeave={() => setIsDragOver(false)}
        onDrop={handleDrop}
      >
        <button
          onClick={() => fileInputRef.current?.click()}
          disabled={uploading}
          className="p-1.5 rounded-lg hover:bg-bg-hover transition-colors text-text-muted hover:text-text-primary shrink-0 mb-0.5"
        >
          <Paperclip size={18} />
        </button>
        <input
          ref={fileInputRef}
          type="file"
          className="hidden"
          multiple
          accept="image/*,.pdf,.txt,.md,.docx,.py,.js,.ts,.rs,.go,.c,.cpp,.java"
          onChange={(e) => e.target.files && handleFiles(e.target.files)}
        />
        <textarea
          ref={textareaRef}
          value={text}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder={isDragOver ? 'Drop files here...' : 'Message Layers...'}
          rows={1}
          className="flex-1 bg-transparent resize-none outline-none text-text-primary placeholder-text-muted text-sm leading-6 max-h-[200px]"
        />
        <button
          onClick={handleSend}
          disabled={isStreaming || (!text.trim() && pendingAttachments.length === 0)}
          className={`p-2 rounded-xl transition-all shrink-0 mb-0.5 ${
            text.trim() || pendingAttachments.length > 0
              ? 'bg-accent hover:bg-accent-hover text-white'
              : 'bg-bg-tertiary text-text-muted'
          } disabled:opacity-50 disabled:cursor-not-allowed`}
        >
          <Send size={16} />
        </button>
      </div>

      {uploading && (
        <div className="text-xs text-text-muted mt-1 px-2 animate-pulse-soft">
          Uploading files...
        </div>
      )}
    </div>
  );
}
