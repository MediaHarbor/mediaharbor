import { useEffect } from 'react';
import { useDownloadStore } from '@/stores/useDownloadStore';
import { useLogStore } from '@/stores/useLogStore';

export function useDownloadEvents() {
  const addOrUpdate = useDownloadStore((s) => s.addOrUpdate);
  const addLog = useLogStore((s) => s.addLog);

  useEffect(() => {
    if (!window.electron) return;

    const cleanups = [
      window.electron.downloads.onInfo((data) => {
        addOrUpdate({
          order: data.order,
          title: data.title ?? 'Downloading...',
          artist: data.artist ?? data.uploader ?? '',
          album: data.album ?? undefined,
          thumbnail: data.thumbnail ?? null,
          platform: data.platform ?? undefined,
          quality: data.quality ? String(data.quality) : undefined,
          progress: 0,
          status: 'downloading',
        });
        addLog({
          order: data.order,
          source: 'download',
          title: `Download started: ${data.title ?? 'Unknown'}`,
          fullLog: `Started downloading "${data.title ?? 'Unknown'}" by ${data.artist ?? data.uploader ?? 'Unknown'}`,
          level: 'info',
        });
      }),
      window.electron.downloads.onProgress((data) => {
        addOrUpdate({
          order: data.order,
          progress: Math.min(Math.round(data.progress ?? 0), 100),
          status: 'downloading',
          ...(data.title != null && { title: data.title }),
          ...(data.thumbnail != null && { thumbnail: data.thumbnail }),
          ...(data.artist != null && { artist: data.artist }),
          ...(data.album != null && { album: data.album }),
        });
      }),
      window.electron.downloads.onComplete((data) => {
        const hasWarnings = !!data.warnings;
        addOrUpdate({
          order: data.order,
          progress: 100,
          status: hasWarnings ? 'error' : 'complete',
          error: data.warnings ?? undefined,
          location: data.location ?? undefined,
        });
        addLog({
          order: data.order,
          source: 'download',
          title: data.title || `Download #${data.order}`,
          fullLog: data.fullLog || data.warnings || 'Completed successfully.',
          level: hasWarnings ? 'warning' : 'info',
        });
      }),
      window.electron.downloads.onError((data) => {
        const order = typeof data === 'object' ? data.order : undefined;
        const error = typeof data === 'object' ? data.error : String(data);
        const fullLog = typeof data === 'object' ? data.fullLog : String(data);
        const title = typeof data === 'object' ? data.title : undefined;
        if (order !== undefined) {
          addOrUpdate({ order, status: 'error', error });
          addLog({
            order,
            source: 'download',
            title: title || `Download #${order}`,
            fullLog: fullLog || error || 'Unknown error',
            level: 'error',
          });
        }
      }),
    ];

    return () => cleanups.forEach((fn) => fn());
  }, [addOrUpdate, addLog]);
}
