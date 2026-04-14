import { useState, useEffect, useRef } from 'react';
import { ChevronDown, Cpu } from 'lucide-react';
import { useSettingsStore } from '../stores/settings';
import { fetchModels } from '../lib/api';
import type { ModelInfo } from '../types';

export function ModelSelector() {
  const { selectedModel, setModel } = useSettingsStore();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    fetchModels()
      .then(setModels)
      .catch(() => {
        setModels([
          { id: 'default', name: 'Default Model', provider: 'layers' },
        ]);
      });
  }, []);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  const currentModel = models.find((m) => m.id === selectedModel) ?? models[0];

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-bg-tertiary hover:bg-bg-hover transition-colors text-sm text-text-secondary hover:text-text-primary"
      >
        <Cpu size={14} />
        <span>{currentModel?.name ?? 'Select Model'}</span>
        <ChevronDown size={14} className={`transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>

      {open && (
        <div className="absolute top-full left-0 mt-1 w-64 bg-bg-secondary border border-border rounded-lg shadow-xl z-50 py-1 animate-fade-in">
          {models.map((m) => (
            <button
              key={m.id}
              onClick={() => { setModel(m.id); setOpen(false); }}
              className={`w-full text-left px-3 py-2 text-sm hover:bg-bg-hover transition-colors flex items-center justify-between ${
                m.id === selectedModel ? 'text-accent' : 'text-text-primary'
              }`}
            >
              <div>
                <div>{m.name}</div>
                <div className="text-xs text-text-muted">{m.provider}</div>
              </div>
              {m.id === selectedModel && (
                <div className="w-1.5 h-1.5 rounded-full bg-accent" />
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
