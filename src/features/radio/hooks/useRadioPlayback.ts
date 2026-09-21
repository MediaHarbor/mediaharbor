import { useCallback } from 'react';
import type { RadioStation } from '@/features/radio/api';
import { stationSubtitle } from '@/features/radio/format';
import { usePlayerStore, type PlayableTrack } from '@/stores/usePlayerStore';

/**
 * A station as a queue entry.
 *
 * The `url` is the station's qualified key, not a stream URL — `RadioPlayback`
 * resolves it, which is what lets a favourite keep playing when the directory is
 * unreachable and what keeps the URL fresh when it is not.
 */
export function stationToTrack(station: RadioStation): PlayableTrack {
  return {
    url: station.key,
    title: station.name,
    artist: stationSubtitle(station),
    album: null,
    thumbnail: station.favicon ?? undefined,
    coverId: station.coverId,
    platform: 'radio',
    mediaType: 'audio',
  };
}

/** Starts one station. */
export function usePlayStation() {
  const setQueue = usePlayerStore((s) => s.setQueue);
  return useCallback((station: RadioStation) => setQueue([stationToTrack(station)], 0), [setQueue]);
}

/** Makes a whole collection the queue, so "next" moves to the next station. */
export function usePlayStations() {
  const setQueue = usePlayerStore((s) => s.setQueue);
  return useCallback(
    (stations: RadioStation[]) => {
      if (stations.length === 0) return;
      setQueue(stations.map(stationToTrack), 0);
    },
    [setQueue]
  );
}

/** The station currently playing, if the player is on a radio track at all. */
export function useNowPlayingKey(): string | null {
  return usePlayerStore((s) => {
    const track = s.queueIndex >= 0 ? s.queue[s.queueIndex] : undefined;
    return track?.platform === 'radio' ? track.url : null;
  });
}
