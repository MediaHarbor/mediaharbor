export interface TourStep {
  id: string;
  selector?: string;
  route?: string;
  title: string;
  body: string;
  placement?: 'right' | 'bottom' | 'left' | 'top' | 'center';
}

export const TOUR_STEPS: TourStep[] = [
  {
    id: 'sidebar',
    selector: '[data-tour-id="sidebar"]',
    title: 'Your sidebar',
    body: 'Everything in MediaHarbor lives here. Let me show you each section.',
    placement: 'right',
  },
  {
    id: 'search',
    selector: '[data-tour-id="nav-search"]',
    route: '/search',
    title: 'Search',
    body: 'Find tracks, albums, and playlists across Spotify, Tidal, Deezer, Qobuz, Apple Music, and YouTube.',
    placement: 'right',
  },
  {
    id: 'downloads',
    selector: '[data-tour-id="nav-downloads"]',
    route: '/downloads',
    title: 'Downloads',
    body: 'Watch active downloads, retry failures, download manually with link, and review what finished.',
    placement: 'right',
  },
  {
    id: 'library',
    selector: '[data-tour-id="nav-library"]',
    route: '/library',
    title: 'Library',
    body: 'All your downloaded music & videos, ready to browse and play offline. Can be unstable for big folders.',
    placement: 'right',
  },
  {
    id: 'updates',
    selector: '[data-tour-id="nav-updates"]',
    route: '/updates',
    title: 'Dependencies',
    body: 'Re-install or update the tools we just set up — Python, FFmpeg, yt-dlp, and rest.',
    placement: 'right',
  },
  {
    id: 'logs',
    selector: '[data-tour-id="nav-logs"]',
    route: '/logs',
    title: 'Logs',
    body: 'Check here first if something looks off. Errors and backend output land in this view.',
    placement: 'right',
  },
  {
    id: 'settings',
    selector: '[data-tour-id="nav-settings"]',
    route: '/settings',
    title: 'Settings',
    body: 'Service credentials, audio quality, file paths, and everything else you can configure.',
    placement: 'right',
  },
  {
    id: 'finish',
    title: "You're ready",
    body: 'Head to Search to find your first track. You can revisit any of this from the sidebar.',
    placement: 'center',
  },
];
