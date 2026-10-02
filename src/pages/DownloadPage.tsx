import { useState, useEffect, useMemo } from 'react';
import { errorMessage } from '@/utils/errors';
import { AnimatePresence, motion } from 'framer-motion';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Slider } from '@/components/ui/slider';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  CircleCheck,
  CircleX,
  X,
  FolderOpen,
  ScrollText,
  Square,
  Copy,
  Trash2,
  TriangleAlert,
} from 'lucide-react';
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
} from '@/components/ui/context-menu';
import { downloadService } from '@/services/ipc/downloads';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { useDownloadStore } from '@/stores/useDownloadStore';
import { useLogStore } from '@/stores/useLogStore';
import { useNavigate } from 'react-router-dom';
import { PlatformIcon } from '@/utils/platforms';
import { cn } from '@/utils/cn';
import { PLATFORM_COLORS, PLATFORM_LABELS, detectPlatform } from '@/utils/platform-data';
import { SKIP_AWARE_PLATFORMS } from '@/utils/constants';
import { qualityLabel, ytMusicSliderQuality } from '@/utils/quality';
import { useQualityOptions } from '@/hooks/useQualityOptions';
import { isBackendAvailable, tauriAPI } from '@/tauri-bridge';
import { copyText } from '@/utils/clipboard';

export default function DownloadPage() {
  const [url, setUrl] = useState('');
  const platform = useMemo(() => detectPlatform(url), [url]);
  const [quality, setQuality] = useState('');
  const [ytMusicQuality, setYtMusicQuality] = useState(10);
  const [mediaKind, setMediaKind] = useState<'audio' | 'video'>('audio');
  const [forceReDownload, setForceReDownload] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const addNotification = useNotificationStore((s) => s.addNotification);
  const { items, remove, cancel, clear } = useDownloadStore();
  const logEntries = useLogStore((s) => s.entries);
  const setHighlight = useLogStore((s) => s.setHighlight);
  const navigate = useNavigate();

  const { options, defaultQuality } = useQualityOptions(platform);
  const platformColor = platform && platform !== 'generic' ? PLATFORM_COLORS[platform] : undefined;
  const supportsKindToggle = platform === 'generic' || platform === 'youtube';

  useEffect(() => {
    setYtMusicQuality(10);
  }, [url]);

  useEffect(() => {
    setQuality((prev) => (options.some((o) => o.value === prev) ? prev : defaultQuality));
  }, [options, defaultQuality]);

  const handleDownload = async () => {
    if (!url.trim()) {
      addNotification({ type: 'error', title: 'Error', message: 'Please enter a URL' });
      return;
    }
    if (!platform) {
      addNotification({ type: 'error', title: 'Error', message: 'Please enter a URL' });
      return;
    }
    if (!isBackendAvailable()) {
      addNotification({ type: 'error', title: 'Error', message: 'Backend not available' });
      return;
    }

    setIsLoading(true);
    try {
      const ytMusicQualityStr = ytMusicSliderQuality(ytMusicQuality);
      if (platform === 'generic') {
        if (mediaKind === 'audio') {
          tauriAPI.downloads.startYouTubeMusic({ url, quality: '' });
        } else {
          tauriAPI.downloads.startGenericVideo({
            url,
            quality: quality || 'bestvideo+bestaudio/best',
          });
        }
      } else if (platform === 'youtube' && mediaKind === 'audio') {
        tauriAPI.downloads.startYouTubeMusic({ url, quality: ytMusicQualityStr });
      } else {
        const effectiveQuality = platform === 'youtubemusic' ? ytMusicQualityStr : quality;
        await downloadService.startDownload({
          platform,
          url,
          quality: effectiveQuality,
          forceRedownload: forceReDownload,
        });
      }
      addNotification({
        type: 'success',
        title: 'Download Started',
        message: `Downloading from ${platform}...`,
      });
      setUrl('');
      setForceReDownload(false);
    } catch (err) {
      addNotification({
        type: 'error',
        title: 'Download Failed',
        message: errorMessage(err),
      });
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="p-6 space-y-6">
      <div className="flex gap-3">
        <Input
          placeholder="Paste URL — YouTube, Spotify, Qobuz, Tidal, Deezer, Apple Music..."
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleDownload()}
          className="flex-1 h-11 text-base"
        />
        <Button
          onClick={handleDownload}
          disabled={isLoading || !url.trim()}
          className="h-11 px-6 transition-colors duration-300"
          style={
            platformColor
              ? { backgroundColor: platformColor, color: '#fff', borderColor: platformColor }
              : undefined
          }
        >
          {isLoading ? 'Starting...' : 'Download'}
        </Button>
      </div>

      {platform && (
        <div
          className="flex flex-wrap items-center gap-4 rounded-lg border bg-card px-5 py-3"
          style={
            platformColor
              ? {
                  borderColor: `${platformColor}55`,
                  borderLeftColor: platformColor,
                  borderLeftWidth: 3,
                }
              : undefined
          }
        >
          <span className="flex items-center gap-2 text-sm text-muted-foreground">
            {platformColor && (
              <span style={{ color: platformColor }} className="flex items-center">
                <PlatformIcon platform={platform} size={15} />
              </span>
            )}
            Detected:{' '}
            <span className="font-semibold text-foreground">
              {PLATFORM_LABELS[platform] ?? platform}
            </span>
          </span>
          {supportsKindToggle && (
            <div className="flex rounded-lg bg-muted/40 p-0.5">
              <button
                type="button"
                onClick={() => setMediaKind('audio')}
                className={cn(
                  'px-3 py-1 rounded-md text-xs font-medium transition-colors',
                  mediaKind === 'audio'
                    ? 'bg-background text-foreground shadow-sm'
                    : 'text-muted-foreground hover:text-foreground'
                )}
              >
                Audio
              </button>
              <button
                type="button"
                onClick={() => setMediaKind('video')}
                className={cn(
                  'px-3 py-1 rounded-md text-xs font-medium transition-colors',
                  mediaKind === 'video'
                    ? 'bg-background text-foreground shadow-sm'
                    : 'text-muted-foreground hover:text-foreground'
                )}
              >
                Video
              </button>
            </div>
          )}
          {supportsKindToggle && mediaKind === 'audio' ? (
            <span className="text-xs text-muted-foreground">
              Extracts audio (uses your yt-dlp audio format)
            </span>
          ) : platform === 'youtubemusic' ? (
            <div className="flex items-center gap-3 min-w-[260px]">
              <span className="text-xs text-muted-foreground shrink-0">Worst</span>
              <Slider
                min={0}
                max={10}
                step={1}
                value={[ytMusicQuality]}
                onValueChange={([v]) => setYtMusicQuality(v)}
                className="flex-1"
              />
              <span className="text-xs text-muted-foreground shrink-0">Best</span>
              <span className="text-xs font-medium w-4 text-center">{ytMusicQuality}</span>
            </div>
          ) : (
            options.length > 0 && (
              <Select value={quality} onValueChange={setQuality}>
                <SelectTrigger className="w-[240px]">
                  <SelectValue placeholder="Select quality" />
                </SelectTrigger>
                <SelectContent>
                  {options.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )
          )}
          {SKIP_AWARE_PLATFORMS.has(platform) && (
            <div className="flex items-center gap-2">
              <Checkbox
                id="force-redownload"
                checked={forceReDownload}
                onCheckedChange={(v) => setForceReDownload(!!v)}
              />
              <Label htmlFor="force-redownload">Force re-download</Label>
            </div>
          )}
        </div>
      )}

      {items.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="font-semibold text-lg">Queue ({items.length})</h2>
            <Button variant="ghost" size="sm" onClick={clear}>
              Clear all
            </Button>
          </div>

          <AnimatePresence initial={false}>
            <div className="grid gap-3">
              {items.map((item) => (
                <ContextMenu key={item.order}>
                  <ContextMenuTrigger asChild>
                    <motion.div
                      initial={{ opacity: 0, y: -8 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, x: 20, transition: { duration: 0.15 } }}
                      transition={{ duration: 0.2 }}
                      className="flex items-center gap-5 rounded-xl border border-border bg-card p-4"
                    >
                      {item.thumbnail ? (
                        <img
                          src={item.thumbnail}
                          alt={item.title}
                          className="h-16 w-16 rounded-lg object-cover shrink-0"
                        />
                      ) : (
                        <div className="h-16 w-16 rounded-lg bg-muted shrink-0" />
                      )}

                      <div className="flex-1 min-w-0 space-y-2">
                        <div>
                          <p className="font-semibold text-base truncate">{item.title}</p>
                          {item.artist && (
                            <p className="text-sm text-muted-foreground truncate">{item.artist}</p>
                          )}
                          {item.album && (
                            <p className="text-xs text-muted-foreground truncate">{item.album}</p>
                          )}
                          <div className="flex flex-wrap items-center gap-2 mt-1">
                            {item.platform && item.platform !== 'generic' && (
                              <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                                <span
                                  style={{
                                    color:
                                      PLATFORM_COLORS[
                                        item.platform as keyof typeof PLATFORM_COLORS
                                      ],
                                  }}
                                >
                                  <PlatformIcon platform={item.platform} size={12} />
                                </span>
                                {PLATFORM_LABELS[item.platform as keyof typeof PLATFORM_LABELS] ??
                                  item.platform}
                              </span>
                            )}
                            {item.quality && (
                              <span className="inline-flex items-center rounded-md bg-muted px-1.5 py-0.5 text-xs font-medium text-muted-foreground">
                                {qualityLabel(item.platform, item.quality)}
                              </span>
                            )}
                          </div>
                        </div>

                        {item.status === 'downloading' && (
                          <div className="space-y-1">
                            <div className="h-2 w-full rounded-full bg-muted overflow-hidden">
                              <div
                                className="h-full rounded-full bg-primary transition-all duration-300"
                                style={{ width: `${item.progress}%` }}
                              />
                            </div>
                            <div className="flex items-center gap-3 text-xs text-muted-foreground">
                              <span>{item.progress}%</span>
                              {item.itemTotal != null && item.itemTotal > 1 && (
                                <span>
                                  {item.itemIndex ?? 0} / {item.itemTotal}
                                </span>
                              )}
                              {item.speed && <span>{item.speed}</span>}
                              {item.eta && <span>{item.eta} left</span>}
                              {item.currentTrack && (
                                <span className="truncate min-w-0 flex-1" title={item.currentTrack}>
                                  {item.currentTrack}
                                </span>
                              )}
                            </div>
                          </div>
                        )}

                        {item.status === 'cancelled' && (
                          <div className="flex items-center gap-2 text-muted-foreground">
                            <Square className="h-4 w-4 shrink-0" />
                            <span className="text-sm font-medium">Cancelled</span>
                          </div>
                        )}

                        {item.status === 'complete' && (
                          <div className="flex items-center gap-2">
                            <CircleCheck className="h-4 w-4 text-green-500" />
                            <span className="text-sm font-medium text-green-500">Complete</span>
                            {item.location && (
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
                                onClick={() => tauriAPI.downloads.showItemInFolder(item.location!)}
                              >
                                <FolderOpen className="h-3 w-3 mr-1" />
                                Show in folder
                              </Button>
                            )}
                          </div>
                        )}

                        {item.status === 'partial' && (
                          <div className="space-y-1">
                            <div className="flex items-center gap-2 text-amber-500">
                              <TriangleAlert className="h-4 w-4 shrink-0" />
                              <span className="text-sm truncate">{item.error}</span>
                            </div>
                            {item.failures && item.failures.length > 0 && (
                              <ul className="text-xs text-muted-foreground space-y-0.5 pl-6">
                                {item.failures.slice(0, 4).map((f) => (
                                  <li key={f} className="truncate" title={f}>
                                    {f}
                                  </li>
                                ))}
                                {item.failures.length > 4 && (
                                  <li>+{item.failures.length - 4} more — see logs</li>
                                )}
                              </ul>
                            )}
                            <div className="flex items-center gap-1">
                              {item.location && (
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
                                  onClick={() =>
                                    tauriAPI.downloads.showItemInFolder(item.location!)
                                  }
                                >
                                  <FolderOpen className="h-3 w-3 mr-1" />
                                  Show in folder
                                </Button>
                              )}
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
                                onClick={() => {
                                  const logEntry = logEntries.find((e) => e.order === item.order);
                                  if (logEntry) setHighlight(logEntry.id);
                                  navigate('/logs');
                                }}
                              >
                                <ScrollText className="h-3 w-3 mr-1" />
                                View Logs
                              </Button>
                            </div>
                          </div>
                        )}

                        {item.status === 'error' && (
                          <div className="space-y-1">
                            <div className="flex items-center gap-2 text-destructive">
                              <CircleX className="h-4 w-4 shrink-0" />
                              <span className="text-sm truncate">{item.error}</span>
                            </div>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
                              onClick={() => {
                                const logEntry = logEntries.find((e) => e.order === item.order);
                                if (logEntry) setHighlight(logEntry.id);
                                navigate('/logs');
                              }}
                            >
                              <ScrollText className="h-3 w-3 mr-1" />
                              View Logs
                            </Button>
                          </div>
                        )}
                      </div>

                      {item.status === 'downloading' && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="shrink-0 text-muted-foreground hover:text-destructive"
                          title="Cancel download"
                          onClick={() => cancel(item.order)}
                        >
                          <Square className="h-4 w-4" />
                        </Button>
                      )}

                      {item.status !== 'downloading' && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="shrink-0"
                          onClick={() => remove(item.order)}
                        >
                          <X className="h-4 w-4" />
                        </Button>
                      )}
                    </motion.div>
                  </ContextMenuTrigger>
                  <ContextMenuContent>
                    {(item.status === 'complete' || item.status === 'partial') && item.location && (
                      <ContextMenuItem
                        onSelect={() => tauriAPI.downloads.showItemInFolder(item.location!)}
                      >
                        <FolderOpen className="h-4 w-4 mr-2" /> Show in folder
                      </ContextMenuItem>
                    )}
                    {item.status === 'error' && (
                      <ContextMenuItem
                        onSelect={() => {
                          const logEntry = logEntries.find((e) => e.order === item.order);
                          if (logEntry) setHighlight(logEntry.id);
                          navigate('/logs');
                        }}
                      >
                        <ScrollText className="h-4 w-4 mr-2" /> View logs
                      </ContextMenuItem>
                    )}
                    {item.status === 'downloading' && (
                      <ContextMenuItem onSelect={() => cancel(item.order)}>
                        <Square className="h-4 w-4 mr-2" /> Cancel download
                      </ContextMenuItem>
                    )}
                    <ContextMenuItem
                      onSelect={() => {
                        void copyText(item.title);
                      }}
                    >
                      <Copy className="h-4 w-4 mr-2" /> Copy title
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem onSelect={() => remove(item.order)}>
                      <Trash2 className="h-4 w-4 mr-2" /> Remove from list
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
              ))}
            </div>
          </AnimatePresence>
        </div>
      )}
    </div>
  );
}
