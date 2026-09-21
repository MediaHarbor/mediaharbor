import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { errorMessage } from '@/utils/errors';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  ArrowLeft,
  Play,
  Disc3,
  Trash2,
  Download,
  Edit2,
  Check,
  X,
  Sparkles,
  ImagePlus,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import {
  ipcPlatformOf,
  toPlayable,
  trackIdFromUrl,
  type ServicePlatform,
} from '@/features/library/api';
import { useStartRadio, useOwnedPlaylists } from '@/features/library/hooks/useLibraryMutations';
import { useCoverUrls, useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import { SaveToggleHeart } from '@/features/library/components/SaveToggleHeart';
import { TrackContextMenu, TrackKebab } from '@/features/library/actions/RowContextMenu';
import { contextUriFor, normalizePlatform } from '@/utils/platform-data';
import { EntityLink } from '@/features/library/components/EntityLink';
import { CoverCropDialog } from '@/features/library/components/CoverCropDialog';
import { formatDurationShort as fmt } from '@/utils/formatters';
import { tauriAPI } from '@/tauri-bridge';

interface PlaylistDto {
  id: number;
  name: string;
  track_count: number;
  cover_ids?: string[];
  description?: string | null;
  owner?: string | null;
}

interface PlaylistTrack {
  path: string;
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  album_key?: string | null;
  artist_id?: string | null;
  duration_secs?: number | null;
  cover_id?: string | null;
}

interface PlaylistDetailViewProps {
  playlistId: number;
  serviceId?: string | null;
  source?: ServicePlatform;
  onBack: () => void;
}

const ROW_GRID = 'grid-cols-[24px_40px_minmax(0,3fr)_minmax(0,2fr)_60px_32px]';

interface PlaylistTrackRowProps {
  t: PlaylistTrack;
  index: number;
  url: string | null;
  size: number;
  start: number;
  isLocal: boolean;
  source: ServicePlatform;
  serviceId?: string | null;
  onPlay: (index: number) => void;
  onRemove: (index: number) => void;
  onMutated: () => void;
}

const PlaylistTrackRow = memo(function PlaylistTrackRow({
  t,
  index,
  url,
  size,
  start,
  isLocal,
  source,
  serviceId,
  onPlay,
  onRemove,
  onMutated,
}: PlaylistTrackRowProps) {
  const trackId = useMemo(() => (!isLocal ? trackIdFromUrl(t.path) : ''), [isLocal, t.path]);
  const canMenu = !isLocal && !!serviceId && !!trackId;
  const [armed, setArmed] = useState(false);
  const normalizedPlatform = useMemo(() => normalizePlatform(source), [source]);
  const arm = useCallback(() => {
    if (canMenu) setArmed(true);
  }, [canMenu]);

  const menuTrack = {
    id: trackId,
    title: t.title ?? null,
    artist: t.artist ?? null,
    album: t.album ?? null,
    url: t.path,
    thumbnail: url,
    albumId: t.album_key ?? null,
    artistId: t.artist_id ?? null,
  };

  const rowInner = (
    <div
      className={`group grid ${ROW_GRID} gap-3 px-3 items-center text-sm hover:bg-card/50 rounded cursor-pointer`}
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        height: size,
        transform: `translateY(${start}px)`,
        contain: 'layout paint',
      }}
      onDoubleClick={() => onPlay(index)}
      onPointerEnter={arm}
      onContextMenu={(e) => {
        if (canMenu && !armed) {
          e.preventDefault();
          setArmed(true);
        }
      }}
    >
      <div className="text-muted-foreground/40 tabular-nums">{index + 1}</div>
      {url ? (
        <img
          src={url}
          alt=""
          loading="lazy"
          decoding="async"
          className="h-8 w-8 rounded object-cover"
        />
      ) : (
        <div className="h-8 w-8 rounded bg-muted flex items-center justify-center">
          <Disc3 className="h-4 w-4 text-muted-foreground/30" />
        </div>
      )}
      <div className="truncate font-medium">{t.title ?? ''}</div>
      <EntityLink
        kind="artist"
        id={t.artist_id}
        platform={normalizedPlatform}
        source={source}
        className="text-muted-foreground/60"
      >
        {t.artist ?? ''}
      </EntityLink>
      <div className="text-right text-muted-foreground/50 tabular-nums">{fmt(t.duration_secs)}</div>
      {isLocal ? (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onRemove(index);
          }}
          className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-destructive/20 text-muted-foreground/60 hover:text-destructive justify-self-end"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
      ) : canMenu ? (
        <div
          className="opacity-0 group-hover:opacity-100 transition-opacity justify-self-end"
          onClick={(e) => e.stopPropagation()}
        >
          {armed && (
            <TrackKebab
              size="sm"
              platform={normalizedPlatform}
              context="playlist"
              playlistId={serviceId ?? undefined}
              track={menuTrack}
              onPlay={() => onPlay(index)}
            />
          )}
        </div>
      ) : (
        <div />
      )}
    </div>
  );

  if (canMenu && armed) {
    return (
      <TrackContextMenu
        platform={normalizedPlatform}
        track={menuTrack}
        context="playlist"
        playlistId={serviceId ?? undefined}
        onPlay={() => onPlay(index)}
        onMutated={onMutated}
      >
        {rowInner}
      </TrackContextMenu>
    );
  }
  return rowInner;
});

export function PlaylistDetailView({
  playlistId,
  serviceId,
  source = 'local',
  onBack,
}: PlaylistDetailViewProps) {
  const [meta, setMeta] = useState<PlaylistDto | null>(null);
  const [tracks, setTracks] = useState<PlaylistTrack[]>([]);
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState('');
  const setQueue = usePlayerStore((s) => s.setQueue);
  const isLocal = source === 'local';
  const { data: ownedPlaylists } = useOwnedPlaylists(isLocal ? null : ipcPlatformOf(source));
  const isOwned = useMemo(
    () => !!serviceId && !!ownedPlaylists?.some((p) => p.serviceId === serviceId),
    [ownedPlaylists, serviceId]
  );
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const parentRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    const fetcher = isLocal
      ? tauriAPI.library.playlists?.get(playlistId)
      : serviceId
        ? tauriAPI.serviceLibrary?.playlist?.(source, serviceId)
        : Promise.resolve(null);
    if (!fetcher) {
      setLoading(false);
      setLoadError('No playlist API available for this source.');
      return;
    }
    fetcher
      .then((r) => {
        if (!r) {
          setLoadError(
            !isLocal && !serviceId
              ? 'This playlist is missing a service id and cannot be opened from outside its tab.'
              : 'Playlist not found.'
          );
          return;
        }
        setMeta(r.playlist as unknown as PlaylistDto);
        setTracks((r.tracks ?? []) as unknown as PlaylistTrack[]);
      })
      .catch((e: unknown) => setLoadError(errorMessage(e)))
      .finally(() => setLoading(false));
  }, [playlistId, source, serviceId, isLocal]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const virtualizer = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 48,
    overscan: 6,
  });
  const virtualItems = virtualizer.getVirtualItems();

  const covers = useVisibleCoverUrls(virtualItems.map((v) => tracks[v.index]?.cover_id));

  const headerCoverId = meta?.cover_ids?.[0] ?? null;
  const headerCovers = useCoverUrls([headerCoverId]);
  const headerCover = headerCoverId ? (headerCovers[headerCoverId] ?? null) : null;

  const tracksRef = useRef(tracks);
  const coverUrlsRef = useRef(covers);
  useEffect(() => {
    tracksRef.current = tracks;
    coverUrlsRef.current = covers;
  });

  const playAll = useCallback(
    (startAt = 0) => {
      const list = tracksRef.current;
      if (list.length === 0) return;
      const platform = isLocal ? 'local' : ipcPlatformOf(source);
      const c = coverUrlsRef.current;
      const playable: PlayableTrack[] = list.map((t) =>
        toPlayable(t, platform, c, { mediaType: 'audio' })
      );
      const contextUri = contextUriFor(source, 'playlist', serviceId);
      setQueue(playable, Math.min(startAt, playable.length - 1), contextUri);
    },
    [isLocal, source, serviceId, setQueue]
  );

  const saveRename = async () => {
    if (!meta || !draftName.trim() || draftName.trim() === meta.name) {
      setEditing(false);
      return;
    }
    await tauriAPI.library.playlists?.rename(meta.id, draftName.trim());
    setEditing(false);
    refresh();
  };

  const removeTrackAt = useCallback(
    (position: number) => {
      if (!meta) return;
      void tauriAPI.library.playlists?.removeTrack(meta.id, position).then(() => refresh());
    },
    [meta, refresh]
  );

  const exportM3u = async () => {
    if (!meta) return;
    const dest = await tauriAPI.settings.openFolder();
    if (!dest) return;
    const safe = meta.name.replace(/[\\/:*?"<>|]/g, '_');
    const path = `${dest}/${safe}.m3u`;
    await tauriAPI.library.playlists?.exportM3u(meta.id, path);
  };

  const canEnhance = !isLocal && normalizePlatform(source) === 'spotify' && !!serviceId && isOwned;
  const canSetCover = canEnhance;
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [cropFile, setCropFile] = useState<File | null>(null);
  const [cropOpen, setCropOpen] = useState(false);
  const [coverError, setCoverError] = useState<string | null>(null);
  const onPickFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0] ?? null;
    e.target.value = '';
    if (f) {
      setCropFile(f);
      setCropOpen(true);
    }
  };
  const submitCover = useCallback(
    async (jpegBase64: string) => {
      if (!serviceId) return;
      setCoverError(null);
      try {
        await tauriAPI.serviceLibrary?.setCover?.(ipcPlatformOf(source), serviceId, jpegBase64);
        refresh();
      } catch (e) {
        setCoverError(errorMessage(e));
        throw e;
      }
    },
    [serviceId, source, refresh]
  );
  const startRadio = useStartRadio();
  const [recs, setRecs] = useState<PlaylistTrack[]>([]);
  const enhance = useCallback(() => {
    if (!serviceId) return;
    startRadio
      .mutateAsync({ platform: ipcPlatformOf(source), seedKind: 'playlist', seedId: serviceId })
      .then((r) => setRecs((r.tracks ?? []) as unknown as PlaylistTrack[]))
      .catch(() => {});
  }, [serviceId, source, startRadio]);
  const recCovers = useVisibleCoverUrls(recs.map((t) => t.cover_id));
  const playRecs = useCallback(
    (startAt = 0) => {
      if (recs.length === 0) return;
      const platform = ipcPlatformOf(source);
      const playable: PlayableTrack[] = recs.map((t) => ({
        url: t.path,
        title: t.title ?? '',
        artist: t.artist ?? '',
        album: t.album ?? null,
        thumbnail: t.cover_id ? (recCovers[t.cover_id] ?? undefined) : undefined,
        coverId: t.cover_id ?? undefined,
        albumId: t.album_key ?? null,
        artistId: t.artist_id ?? null,
        mediaType: 'audio' as MediaType,
        platform,
      }));
      setQueue(playable, Math.min(startAt, playable.length - 1), null);
    },
    [recs, source, recCovers, setQueue]
  );

  if (!meta) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onClick={onBack} className="gap-2">
          <ArrowLeft className="h-4 w-4" />
          Back
        </Button>
        <div className="py-16 text-center text-sm whitespace-pre-wrap">
          {loading ? (
            <span className="text-muted-foreground/50">Loading playlist…</span>
          ) : (
            <span className="text-red-400/80">{loadError ?? 'Playlist unavailable.'}</span>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 min-h-0 flex flex-col gap-4">
      <div className="shrink-0 space-y-4">
        <Button variant="ghost" size="sm" onClick={onBack} className="gap-2">
          <ArrowLeft className="h-4 w-4" />
          Back
        </Button>

        <div className="flex gap-6 items-end">
          <div className="shrink-0">
            <button
              type="button"
              disabled={!canSetCover}
              onClick={() => canSetCover && fileInputRef.current?.click()}
              className="group relative h-48 w-48 rounded-lg overflow-hidden shadow-xl block"
            >
              {headerCover ? (
                <img
                  src={headerCover}
                  alt={meta.name}
                  loading="lazy"
                  className="h-48 w-48 rounded-lg object-cover"
                />
              ) : (
                <div className="h-48 w-48 rounded-lg bg-gradient-to-br from-muted to-muted/60 flex items-center justify-center">
                  <Disc3 className="h-16 w-16 text-muted-foreground/20" />
                </div>
              )}
              {canSetCover && (
                <div className="absolute inset-0 hidden group-hover:flex items-center justify-center bg-black/50 text-white text-sm font-medium gap-2">
                  <ImagePlus className="h-5 w-5" />
                  Change cover
                </div>
              )}
            </button>
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              className="hidden"
              onChange={onPickFile}
            />
            {coverError && <p className="mt-2 max-w-48 text-xs text-destructive">{coverError}</p>}
          </div>

          <div className="flex-1 min-w-0 pb-1 space-y-4">
            <div className="flex items-center gap-3">
              {editing ? (
                <>
                  <Input
                    autoFocus
                    value={draftName}
                    onChange={(e) => setDraftName(e.target.value)}
                    className="max-w-md text-2xl font-bold h-12"
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') saveRename();
                      if (e.key === 'Escape') setEditing(false);
                    }}
                  />
                  <Button size="icon" onClick={saveRename}>
                    <Check className="h-4 w-4" />
                  </Button>
                  <Button size="icon" variant="ghost" onClick={() => setEditing(false)}>
                    <X className="h-4 w-4" />
                  </Button>
                </>
              ) : (
                <div className="min-w-0">
                  <p className="text-xs font-medium uppercase tracking-widest text-muted-foreground/60 mb-1">
                    Playlist
                  </p>
                  <div className="flex items-center gap-3">
                    <h1 className="text-3xl font-bold truncate leading-tight">{meta.name}</h1>
                    {isLocal && (
                      <Button
                        size="icon"
                        variant="ghost"
                        onClick={() => {
                          setDraftName(meta.name);
                          setEditing(true);
                        }}
                      >
                        <Edit2 className="h-4 w-4" />
                      </Button>
                    )}
                  </div>
                </div>
              )}
            </div>

            {meta.description && (
              <p className="text-sm text-muted-foreground/70 line-clamp-3 max-w-2xl">
                {meta.description}
              </p>
            )}

            <div className="flex items-center gap-2 text-sm text-muted-foreground/60">
              {meta.owner && <span className="truncate">{meta.owner}</span>}
              {meta.owner && <span>·</span>}
              <span>
                {tracks.length} song{tracks.length !== 1 ? 's' : ''}
              </span>
            </div>

            <div className="flex items-center gap-2">
              <Button onClick={() => playAll(0)} className="gap-2" disabled={tracks.length === 0}>
                <Play className="h-4 w-4" />
                Play
              </Button>
              {isLocal && (
                <Button
                  variant="outline"
                  onClick={exportM3u}
                  className="gap-2"
                  disabled={tracks.length === 0}
                >
                  <Download className="h-4 w-4" />
                  Export .m3u
                </Button>
              )}
              {canEnhance && (
                <Button
                  variant="outline"
                  onClick={enhance}
                  className="gap-2"
                  disabled={startRadio.isPending}
                >
                  <Sparkles className="h-4 w-4" />
                  {startRadio.isPending ? 'Finding…' : 'Enhance'}
                </Button>
              )}
              {!isLocal && serviceId && (
                <SaveToggleHeart
                  platform={ipcPlatformOf(source)}
                  kind="playlist"
                  id={serviceId}
                  size="md"
                  visibility="always"
                  label="Follow playlist"
                />
              )}
            </div>
          </div>
        </div>
      </div>

      {tracks.length === 0 && recs.length === 0 ? (
        <div className="text-center py-16 text-muted-foreground/50 text-sm">
          No tracks. Add some from the Tracks tab.
        </div>
      ) : (
        <div ref={parentRef} className="flex-1 min-h-0 overflow-y-auto">
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualItems.map((vi) => {
              const t = tracks[vi.index];
              if (!t) return null;
              const url = t.cover_id ? (covers[t.cover_id] ?? null) : null;
              return (
                <PlaylistTrackRow
                  key={`${t.path}-${vi.index}`}
                  t={t}
                  index={vi.index}
                  url={url}
                  size={vi.size}
                  start={vi.start}
                  isLocal={isLocal}
                  source={source}
                  serviceId={serviceId}
                  onPlay={playAll}
                  onRemove={removeTrackAt}
                  onMutated={refresh}
                />
              );
            })}
          </div>

          {recs.length > 0 && (
            <div className="pt-6">
              <h2 className="px-2 pb-2 text-sm font-semibold text-muted-foreground/80">
                Recommended
              </h2>
              {recs.map((t, i) => {
                const url = t.cover_id ? (recCovers[t.cover_id] ?? null) : null;
                return (
                  <div
                    key={`rec-${t.path}-${i}`}
                    onDoubleClick={() => playRecs(i)}
                    className="grid grid-cols-[2rem_1fr_auto] items-center gap-3 px-2 h-12 rounded-lg hover:bg-muted/40 cursor-pointer"
                  >
                    <div className="text-muted-foreground/40 tabular-nums">{i + 1}</div>
                    <div className="flex items-center gap-3 min-w-0">
                      {url ? (
                        <img
                          src={url}
                          alt=""
                          loading="lazy"
                          className="h-8 w-8 rounded object-cover"
                        />
                      ) : (
                        <div className="h-8 w-8 rounded bg-muted" />
                      )}
                      <div className="min-w-0">
                        <div className="truncate font-medium">{t.title ?? ''}</div>
                        <div className="truncate text-sm text-muted-foreground/60">
                          {t.artist ?? ''}
                        </div>
                      </div>
                    </div>
                    <div className="text-right text-muted-foreground/50 tabular-nums">
                      {fmt(t.duration_secs ?? undefined)}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      <CoverCropDialog
        open={cropOpen}
        onOpenChange={setCropOpen}
        file={cropFile}
        onConfirm={submitCover}
      />
    </div>
  );
}
