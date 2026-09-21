import { useEffect, useMemo, useState } from 'react';
import type { Platform, QualityOption } from '@/types';
import { resolveQualityOptions, savedDefaultQuality } from '@/utils/quality';
import { useAppSettings, useQualityContext } from '@/hooks/useAppSettings';
import { isSpotifyFree, tauriAPI } from '@/tauri-bridge';

/** Stable identity for "no platform yet", so consumers' memos do not churn. */
const NO_OPTIONS: QualityOption[] = [];

/**
 * The quality ladder for one platform and the tier a download should start on.
 *
 * The paste-URL page and the quality dialog both need this, and each had written it
 * out: the same free-account probe, the same backend ternary, the same options memo
 * and the same `saved ?? first ?? ''` fallback. They also disagreed about *how* — one
 * seeded through an effect, the other derived — so the two could land on different
 * qualities for the same track.
 *
 * `enabled` is for the dialog, which should only probe while it is open.
 */
export function useQualityOptions(platform: Platform | string | null, enabled = true) {
  const { spotifyBackend, appleBackend, appleWrapper } = useQualityContext();
  const { data: settings } = useAppSettings();
  const [spotifyFree, setSpotifyFree] = useState(false);

  useEffect(() => {
    if (!enabled || platform !== 'spotify') return;
    let cancelled = false;
    tauriAPI.spotifyAccount
      ?.getStatus()
      .then((status) => {
        if (cancelled) return;
        setSpotifyFree(status.loggedIn ? isSpotifyFree(status.profile) : false);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [enabled, platform]);

  const backend =
    platform === 'spotify' ? spotifyBackend : platform === 'applemusic' ? appleBackend : null;

  const options = useMemo(
    () =>
      platform
        ? resolveQualityOptions(platform, backend, { spotifyFree, appleWrapper })
        : NO_OPTIONS,
    [platform, backend, spotifyFree, appleWrapper]
  );

  const defaultQuality = useMemo(
    () => savedDefaultQuality(platform ?? '', options, settings) ?? options[0]?.value ?? '',
    [platform, options, settings]
  );

  return { options, defaultQuality, spotifyFree, backend };
}
