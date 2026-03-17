import { useLogStore, type LogSource } from '@/stores/useLogStore';
import { useNotificationStore } from '@/stores/useNotificationStore';

interface LogOptions {
  notify?: boolean;
  duration?: number;
  order?: number;
}

function addLog(
  level: 'info' | 'warning' | 'error',
  source: LogSource,
  title: string,
  detail: string,
  options: LogOptions = {}
) {
  const { notify = false, duration, order } = options;

  useLogStore.getState().addLog({
    source,
    title,
    fullLog: detail,
    level,
    ...(order !== undefined ? { order } : {}),
  });

  if (notify) {
    const typeMap = { info: 'info', warning: 'warning', error: 'error' } as const;
    useNotificationStore.getState().addNotification({
      type: typeMap[level],
      title,
      message: detail,
      ...(duration !== undefined ? { duration } : {}),
    });
  }
}

export function logInfo(source: LogSource, title: string, detail: string, options?: LogOptions) {
  addLog('info', source, title, detail, options);
}

export function logWarning(source: LogSource, title: string, detail: string, options?: LogOptions) {
  addLog('warning', source, title, detail, options);
}

export function logError(source: LogSource, title: string, detail: string, options?: LogOptions) {
  addLog('error', source, title, detail, { notify: true, ...options });
}
