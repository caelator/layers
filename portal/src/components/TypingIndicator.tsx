export function TypingIndicator() {
  return (
    <div className="flex items-center gap-1.5 px-4 py-3">
      <div className="flex items-center gap-1">
        <span className="typing-dot w-2 h-2 rounded-full bg-accent" />
        <span className="typing-dot w-2 h-2 rounded-full bg-accent" />
        <span className="typing-dot w-2 h-2 rounded-full bg-accent" />
      </div>
    </div>
  );
}
