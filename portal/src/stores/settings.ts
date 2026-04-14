import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { ModelConfig, ConfigPreset } from '../types';

const defaultModelConfig: ModelConfig = {
  temperature: 0.7,
  max_tokens: 4096,
  top_p: 1.0,
  frequency_penalty: 0,
  presence_penalty: 0,
  context_window: 128000,
  thinking: 'off',
};

interface SettingsState {
  theme: 'dark' | 'light';
  selectedModel: string;
  apiEndpoint: string;
  modelConfig: ModelConfig;
  systemPrompt: string;
  presets: ConfigPreset[];
  activePreset: string | null;
  workspacePath: string;
  timezone: string;
  logLevel: string;

  toggleTheme: () => void;
  setModel: (model: string) => void;
  setApiEndpoint: (endpoint: string) => void;
  setModelConfig: (config: Partial<ModelConfig>) => void;
  setSystemPrompt: (prompt: string) => void;
  savePreset: (name: string) => void;
  loadPreset: (id: string) => void;
  deletePreset: (id: string) => void;
  setWorkspacePath: (path: string) => void;
  setTimezone: (tz: string) => void;
  setLogLevel: (level: string) => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, _get) => ({
      theme: 'dark',
      selectedModel: 'default',
      apiEndpoint: 'http://127.0.0.1:18791',
      modelConfig: defaultModelConfig,
      systemPrompt: '',
      presets: [],
      activePreset: null,
      workspacePath: '',
      timezone: 'America/Bogota',
      logLevel: 'info',

      toggleTheme: () => set((s) => {
        const next = s.theme === 'dark' ? 'light' : 'dark';
        document.documentElement.classList.toggle('light', next === 'light');
        return { theme: next };
      }),

      setModel: (model) => set({ selectedModel: model }),
      setApiEndpoint: (endpoint) => set({ apiEndpoint: endpoint }),
      setModelConfig: (config) => set((s) => ({ modelConfig: { ...s.modelConfig, ...config } })),
      setSystemPrompt: (prompt) => set({ systemPrompt: prompt }),
      setWorkspacePath: (path) => set({ workspacePath: path }),
      setTimezone: (tz) => set({ timezone: tz }),
      setLogLevel: (level) => set({ logLevel: level }),

      savePreset: (name) => set((s) => {
        const id = crypto.randomUUID();
        const preset: ConfigPreset = {
          id,
          name,
          model_id: s.selectedModel,
          config: { ...s.modelConfig },
          system_prompt: s.systemPrompt,
        };
        return { presets: [...s.presets, preset], activePreset: id };
      }),

      loadPreset: (id) => set((s) => {
        const preset = s.presets.find((p) => p.id === id);
        if (!preset) return {};
        return {
          activePreset: id,
          selectedModel: preset.model_id,
          modelConfig: { ...preset.config },
          systemPrompt: preset.system_prompt ?? '',
        };
      }),

      deletePreset: (id) => set((s) => ({
        presets: s.presets.filter((p) => p.id !== id),
        activePreset: s.activePreset === id ? null : s.activePreset,
      })),
    }),
    { name: 'layers-settings' }
  )
);
