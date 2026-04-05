import { useState, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { CheckCircle, XCircle, X, FolderOpen, ScrollText, Square } from 'lucide-react';
import { downloadService } from '@/services/ipc/downloads';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { useDownloadStore } from '@/stores/useDownloadStore';
import { useLogStore } from '@/stores/useLogStore';
import { useNavigate } from 'react-router-dom';
import { PlatformIcon } from '@/utils/platforms';
import { PLATFORM_COLORS, PLATFORM_LABELS, detectPlatform } from '@/utils/platform-data';
import { QUALITY_OPTIONS, PLAYLIST_PLATFORMS } from '@/utils/constants';
import type { Platform, OrpheusPlatform } from '@/types';

export default function DownloadPage() {
  const [url, setUrl] = useState('');
  const [platform, setPlatform] = useState<Platform | OrpheusPlatform | 'generic' | null>(null);
  const [quality, setQuality] = useState('');
  const [isPlaylist, setIsPlaylist] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const addNotification = useNotificationStore((s) => s.addNotification);
  const { items, remove, cancel, clear } = useDownloadStore();
  const logEntries = useLogStore((s) => s.entries);
  const setHighlight = useLogStore((s) => s.setHighlight);
  const navigate = useNavigate();

  const options = platform && platform !== 'generic' ? QUALITY_OPTIONS[platform] || [] : [];
  const showPlaylist = platform ? PLAYLIST_PLATFORMS.includes(platform) : false;
  const platformColor = platform && platform !== 'generic' ? PLATFORM_COLORS[platform] : undefined;

  useEffect(() => {
    const detected = detectPlatform(url);
    setPlatform(detected);
    setIsPlaylist(false);
    if (detected && detected !== 'generic' && QUALITY_OPTIONS[detected]?.length) {
      setQuality(QUALITY_OPTIONS[detected][0].value);
    } else {
      setQuality('');
    }
  }, [url]);

  const handleDownload = async () => {
    if (!url.trim()) {
      addNotification({ type: 'error', title: 'Error', message: 'Please enter a URL' });
      return;
    }
    if (!platform) {
      addNotification({ type: 'error', title: 'Error', message: 'Please enter a URL' });
      return;
    }
    if (!window.electron) {
      addNotification({ type: 'error', title: 'Error', message: 'Not running in Electron' });
      return;
    }

    setIsLoading(true);
    try {
      if (platform === 'generic') {
        window.electron.downloads.startGenericVideo({ url, quality: quality || 'bestvideo+bestaudio' });
      } else if (isPlaylist) {
        switch (platform) {
          case 'youtube':
            window.electron.downloads.startYouTubeVideo({ url, quality, isPlaylist: true });
            break;
          case 'youtubemusic':
            window.electron.downloads.startYouTubeMusic({ url, quality }, true);
            break;
          case 'spotify':
            window.electron.downloads.startSpotify({ url, quality });
            break;
          case 'qobuz':
            window.electron.downloads.startQobuz({ url, quality });
            break;
          case 'tidal':
            window.electron.downloads.startTidal({ url, quality });
            break;
          case 'deezer':
            window.electron.downloads.startDeezer({ url, quality });
            break;
          case 'applemusic':
            window.electron.downloads.startAppleMusic({ url, quality });
            break;
        }
      } else {
        await downloadService.startDownload({ platform, url, quality });
      }
      addNotification({ type: 'success', title: 'Download Started', message: `Downloading from ${platform}...` });
      setUrl('');
    } catch (err) {
      addNotification({ type: 'error', title: 'Download Failed', message: err instanceof Error ? err.message : String(err) });
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
          style={platformColor ? { backgroundColor: platformColor, color: '#fff', borderColor: platformColor } : undefined}
        >
          {isLoading ? 'Starting...' : 'Download'}
        </Button>
      </div>

      {platform && (
        <div
          className="flex flex-wrap items-center gap-4 rounded-lg border bg-card px-5 py-3"
          style={platformColor ? { borderColor: `${platformColor}55`, borderLeftColor: platformColor, borderLeftWidth: 3 } : undefined}
        >
          <span className="flex items-center gap-2 text-sm text-muted-foreground">
            {platformColor && (
              <span style={{ color: platformColor }} className="flex items-center">
                <PlatformIcon platform={platform} size={15} />
              </span>
            )}
            Detected: <span className="font-semibold text-foreground">{PLATFORM_LABELS[platform] ?? platform}</span>
          </span>
          {options.length > 0 && (
            <Select value={quality} onValueChange={setQuality}>
              <SelectTrigger className="w-[240px]">
                <SelectValue placeholder="Select quality" />
              </SelectTrigger>
              <SelectContent>
                {options.map((o) => (
                  <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          {showPlaylist && (
            <div className="flex items-center gap-2">
              <Checkbox id="playlist" checked={isPlaylist} onCheckedChange={(v) => setIsPlaylist(!!v)} />
              <Label htmlFor="playlist">Playlist</Label>
            </div>
          )}
        </div>
      )}

      {items.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="font-semibold text-lg">Queue ({items.length})</h2>
            <Button variant="ghost" size="sm" onClick={clear}>Clear all</Button>
          </div>

          <AnimatePresence initial={false}>
          <div className="grid gap-3">
            {items.map((item) => (
              <motion.div
                key={item.order}
                layout
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
                          <span style={{ color: PLATFORM_COLORS[item.platform as keyof typeof PLATFORM_COLORS] }}>
                            <PlatformIcon platform={item.platform} size={12} />
                          </span>
                          {PLATFORM_LABELS[item.platform as keyof typeof PLATFORM_LABELS] ?? item.platform}
                        </span>
                      )}
                      {item.quality && (
                        <span className="inline-flex items-center rounded-md bg-muted px-1.5 py-0.5 text-xs font-medium text-muted-foreground">
                          {item.quality}
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
                      <p className="text-xs text-muted-foreground">{item.progress}%</p>
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
                      <CheckCircle className="h-4 w-4 text-green-500" />
                      <span className="text-sm font-medium text-green-500">Complete</span>
                      {item.location && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
                          onClick={() => window.electron?.downloads.showItemInFolder(item.location!)}
                        >
                          <FolderOpen className="h-3 w-3 mr-1" />
                          Show in folder
                        </Button>
                      )}
                    </div>
                  )}

                  {item.status === 'error' && (
                    <div className="space-y-1">
                      <div className="flex items-center gap-2 text-destructive">
                        <XCircle className="h-4 w-4 shrink-0" />
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

                {(item.status === 'complete' || item.status === 'error' || item.status === 'cancelled') && (
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
            ))}
          </div>
          </AnimatePresence>
        </div>
      )}
    </div>
  );
}
