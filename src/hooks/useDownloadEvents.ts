import { useEffect } from 'react';
import { useDownloadStore } from '@/stores/useDownloadStore';
import { useLogStore } from '@/stores/useLogStore';
import { tauriAPI } from '@/tauri-bridge';

export function useDownloadEvents() {
  const addOrUpdate = useDownloadStore((s) => s.addOrUpdate);
  const addLog = useLogStore((s) => s.addLog);

  useEffect(() => {
    const cleanups = [
      tauriAPI.downloads.onInfo((data) => {
        addOrUpdate({
          order: data.order,
          title: data.title ?? 'Downloading...',
          artist: data.artist ?? data.uploader ?? '',
          album: data.album ?? undefined,
          thumbnail: data.thumbnail ?? null,
          platform: data.platform ?? undefined,
          quality: data.quality ? String(data.quality) : undefined,
        });
        addLog({
          order: data.order,
          source: 'download',
          title: `Download started: ${data.title ?? 'Unknown'}`,
          fullLog: `Started downloading "${data.title ?? 'Unknown'}" by ${data.artist ?? data.uploader ?? 'Unknown'}`,
          level: 'info',
        });
      }),
      tauriAPI.downloads.onProgress((data) => {
        addOrUpdate({
          order: data.order,
          progress: Math.min(Math.round(data.progress ?? 0), 100),
          status: 'downloading',
          ...(data.title != null && { title: data.title }),
          ...(data.thumbnail != null && { thumbnail: data.thumbnail }),
          ...(data.artist != null && { artist: data.artist }),
          ...(data.album != null && { album: data.album }),
          ...(data.speed != null && { speed: data.speed }),
          ...(data.eta != null && { eta: data.eta }),
          ...(data.itemIndex != null && { itemIndex: data.itemIndex }),
          ...(data.itemTotal != null && { itemTotal: data.itemTotal }),
          ...(data.currentTrack != null && { currentTrack: data.currentTrack }),
          ...(data.quality != null && { quality: data.quality }),
        });
      }),
      tauriAPI.downloads.onComplete((data) => {
        const hasWarnings = !!data.warnings;
        addOrUpdate({
          order: data.order,
          progress: 100,
          status: hasWarnings ? 'partial' : 'complete',
          error: data.warnings ?? undefined,
          failures: hasWarnings
            ? (data.fullLog ?? '').split('\n').filter((l) => l.startsWith('✗'))
            : undefined,
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
      tauriAPI.downloads.onError((data) => {
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
