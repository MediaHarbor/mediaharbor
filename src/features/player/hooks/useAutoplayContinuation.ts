import { useEffect } from 'react';
import { errorMessage } from '@/utils/errors';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { trackIdFromUrl } from '@/features/library/api';
import { radioTracksToQueue } from '@/features/library/actions/shared';
import { serviceSourceOf } from '@/utils/platform-data';
import { logWarning } from '@/utils/logger';
import { isBackendAvailable, tauriAPI, type RadioResult } from '@/tauri-bridge';

export function useAutoplayContinuation() {
  const queue = usePlayerStore((s) => s.queue);
  const queueIndex = usePlayerStore((s) => s.queueIndex);
  const repeat = usePlayerStore((s) => s.repeat);
  const autoplayEnabled = usePlayerStore((s) => s.autoplayEnabled);
  const autoplayLastSeed = usePlayerStore((s) => s.autoplayLastSeed);
  const autoplayStation = usePlayerStore((s) => s.autoplayStation);
  const setAutoplayLastSeed = usePlayerStore((s) => s.setAutoplayLastSeed);
  const setAutoplayStation = usePlayerStore((s) => s.setAutoplayStation);
  const appendToQueue = usePlayerStore((s) => s.appendToQueue);

  useEffect(() => {
    if (!autoplayEnabled) return;
    if (repeat === 'all') return;
    if (queueIndex < 0) return;
    const isNearEnd = queueIndex >= queue.length - 2;
    if (!isNearEnd) return;
    const current = queue[queueIndex];
    if (!current) return;
    const platform = serviceSourceOf(current.platform);
    if (!platform || platform === 'local') return;
    const seedId = trackIdFromUrl(current.url);
    if (!seedId) return;
    if (!isBackendAvailable()) return;

    const station = autoplayStation?.platform === platform ? autoplayStation : null;
    const seedKey = `${platform}:${seedId}`;
    if (!station && seedKey === autoplayLastSeed) return;

    const carryPlatform = current.platform ?? platform;

    if (station) {
      setAutoplayStation(null);
    } else {
      setAutoplayLastSeed(seedKey);
    }

    void (async () => {
      try {
        const result: RadioResult = station
          ? await tauriAPI.serviceLibrary.radioContinue({
              platform,
              continuation: station.continuation,
            })
          : await tauriAPI.serviceLibrary.radioFor({
              platform,
              seedKind: 'track',
              seedId,
            });

        const nextToken = result?.continuation;
        if (nextToken && nextToken !== station?.continuation) {
          setAutoplayStation({ platform, continuation: nextToken });
        }
        if (!result?.tracks?.length) return;

        const existingUrls = new Set(queue.map((t) => t.url));
        const newPlayable = (await radioTracksToQueue(result, carryPlatform)).filter(
          (t) => !existingUrls.has(t.url)
        );
        if (newPlayable.length === 0) return;
        appendToQueue(newPlayable);
      } catch (e) {
        logWarning(
          'playback',
          station ? 'Queue continuation failed' : 'Autoplay continuation failed',
          `${platform}/${seedId}: ${errorMessage(e)}`
        );
      }
    })();
  }, [
    queue,
    queueIndex,
    repeat,
    autoplayEnabled,
    autoplayLastSeed,
    autoplayStation,
    setAutoplayLastSeed,
    setAutoplayStation,
    appendToQueue,
  ]);
}
