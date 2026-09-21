import type { PlayableTrack } from '@/stores/usePlayerStore';
import type { UndecodableStream } from '@/tauri-bridge';
import { PLATFORM_LABELS, toClientPlatform } from '@/utils/platform-data';

/**
 * The public tracker, not the beta repo — this reaches end users, and everything
 * else that points them somewhere (`HelpPage`, the metainfo `bugtracker` URL)
 * uses the same one.
 */
export const ISSUE_URL = 'https://github.com/MediaHarbor/mediaharbor/issues/new';

export type ReportTrack = Pick<PlayableTrack, 'url' | 'platform'>;

/**
 * Where the audio came from. A service track is named with its URL so the
 * failure can be reproduced; a local file only by its extension, because the
 * path carries the user's name. A track with no platform is treated as local
 * for the same reason.
 */
function sourceOf(track: ReportTrack | undefined): { line: string; subject: string } | null {
  if (!track) return null;
  const platform: string | null = track.platform ? toClientPlatform(track.platform) : null;
  if (!platform || platform === 'local') {
    const name = track.url.split(/[\\/]/).pop()?.split('?')[0] ?? '';
    const dot = name.lastIndexOf('.');
    const ext = dot > 0 ? name.slice(dot).toLowerCase() : '';
    return { line: ext ? `Local file (${ext})` : 'Local file', subject: 'local file' };
  }
  const label = PLATFORM_LABELS[platform] ?? platform;
  return { line: `${label} · ${track.url}`, subject: `${label} track` };
}

/**
 * The report for a track the native player could not decode: pasteable into an
 * issue as-is, with a line only when there is something to put on it — every
 * one is a fact a triager would otherwise have to ask for.
 */
export function buildUndecodableReport(
  u: UndecodableStream,
  track: ReportTrack | undefined
): { text: string; issueUrl: string } {
  const source = sourceOf(track);
  const codec = [
    u.codec,
    u.sampleRate ? `${u.sampleRate} Hz` : null,
    u.channels ? `${u.channels} ch` : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const rows: [string, string | null][] = [
    ['Error', u.detail],
    ['Source', source?.line ?? null],
    ['Container', u.container],
    ['Codec', codec || null],
    ['App', u.app],
  ];
  const text = rows
    .filter((row): row is [string, string] => Boolean(row[1]))
    .map(([key, value]) => `${`${key}:`.padEnd(11)}${value}`)
    .join('\n');

  const fullTitle = `Can't play ${source?.subject ?? 'track'}: ${u.detail}`;
  const title = fullTitle.length > 120 ? `${fullTitle.slice(0, 117)}...` : fullTitle;
  const body = `\`\`\`\n${text}\n\`\`\``;
  const issueUrl = `${ISSUE_URL}?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;

  return { text, issueUrl };
}
