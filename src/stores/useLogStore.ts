import { create } from 'zustand';

export type LogSource =
  | 'download' | 'playback' | 'search' | 'settings'
  | 'system' | 'install' | 'app'
  | 'mediascanner' | 'filewatcher'
  | 'qobuz' | 'deezer' | 'tidal' | 'gam';

export interface LogEntry {
  id: string;
  order?: number;
  source: LogSource;
  title: string;
  timestamp: number;
  fullLog: string;
  level: 'info' | 'warning' | 'error';
}

interface LogState {
  entries: LogEntry[];
  highlightId: string | null;
  addLog: (entry: Omit<LogEntry, 'id' | 'timestamp'>) => void;
  setHighlight: (id: string | null) => void;
  clearLogs: () => void;
}

let logCounter = 0;

export const useLogStore = create<LogState>((set) => ({
  entries: [],
  highlightId: null,

  addLog: (entry) =>
    set((state) => ({
      entries: [
        {
          ...entry,
          id: `log-${++logCounter}`,
          timestamp: Date.now(),
        },
        ...state.entries,
      ].slice(0, 1000),
    })),

  setHighlight: (id) => set({ highlightId: id }),

  clearLogs: () => set({ entries: [], highlightId: null }),
}));
