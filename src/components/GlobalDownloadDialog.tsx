import { QualitySelector } from '@/components/QualitySelector';
import { errorMessage } from '@/utils/errors';
import { useDownloadDialogStore } from '@/stores/useDownloadDialogStore';
import { downloadService } from '@/services/ipc/downloads';
import { useNotificationStore } from '@/stores/useNotificationStore';

export function GlobalDownloadDialog() {
  const open = useDownloadDialogStore((s) => s.open);
  const request = useDownloadDialogStore((s) => s.request);
  const close = useDownloadDialogStore((s) => s.close);
  const addNotification = useNotificationStore((s) => s.addNotification);

  if (!request) return null;
  const label = request.title ?? request.album ?? 'this item';

  return (
    <QualitySelector
      open={open}
      onClose={close}
      platform={request.platform}
      title={label}
      onConfirm={(quality, forceRedownload) => {
        downloadService
          .startDownload({
            platform: request.platform,
            url: request.url,
            quality,
            type: request.kind,
            title: request.title ?? undefined,
            artist: request.artist ?? undefined,
            album: request.album ?? undefined,
            thumbnail: request.thumbnail ?? null,
            forceRedownload,
          })
          .then(() => {
            addNotification({ type: 'success', title: 'Download started', message: label });
          })
          .catch((err) => {
            addNotification({
              type: 'error',
              title: 'Download failed',
              message: errorMessage(err),
            });
          });
      }}
    />
  );
}
