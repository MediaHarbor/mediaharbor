import { useState } from 'react';
import {
  Search,
  Download,
  Library,
  Settings,
  Keyboard,
  Disc3,
  ChevronRight,
  HelpCircle,
  Heart,
  GitFork,
} from 'lucide-react';
import { cn } from '@/utils/cn';

function FaqItem({ question, children }: { question: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border-b border-border last:border-0">
      <button
        type="button"
        className="w-full flex items-center justify-between py-3 text-left text-sm font-medium text-foreground gap-4"
        onClick={() => setOpen((o) => !o)}
      >
        <span>{question}</span>
        <ChevronRight
          className={cn(
            'h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-150',
            open && 'rotate-90'
          )}
        />
      </button>
      {open && (
        <div className="pb-4 text-sm text-muted-foreground space-y-2 pr-4">{children}</div>
      )}
    </div>
  );
}

function FaqGroup({ title, icon: Icon, children }: { title: string; icon: React.ElementType; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-muted/30">
        <Icon className="h-4 w-4 text-muted-foreground" />
        <h3 className="font-semibold text-sm">{title}</h3>
      </div>
      <div className="px-4">{children}</div>
    </div>
  );
}

interface Section {
  id: string;
  icon: React.ElementType;
  title: string;
}

const sections: Section[] = [
  { id: 'faq',        icon: HelpCircle, title: 'FAQ' },
  { id: 'search',     icon: Search,     title: 'Search' },
  { id: 'downloads',  icon: Download,   title: 'Downloads' },
  { id: 'library',    icon: Library,    title: 'Library' },
  { id: 'shortcuts',  icon: Keyboard,   title: 'Keyboard Shortcuts' },
];

function SectionAnchor({ id }: { id: string }) {
  return <div id={id} className="-mt-4 pt-4" />;
}

function H2({ icon: Icon, children }: { icon: React.ElementType; children: React.ReactNode }) {
  return (
    <h2 className="flex items-center gap-2 text-lg font-semibold text-foreground mt-10 mb-4 pb-2 border-b border-border">
      <Icon className="h-5 w-5 text-muted-foreground" />
      {children}
    </h2>
  );
}

function Tip({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex gap-2 items-start rounded-lg bg-muted/50 border border-border px-4 py-3 text-sm text-muted-foreground">
      <ChevronRight className="h-4 w-4 mt-0.5 shrink-0 text-primary" />
      <span>{children}</span>
    </div>
  );
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex items-center gap-0.5 rounded border border-border bg-muted px-1.5 py-0.5 text-xs font-mono text-foreground shadow-sm">
      {children}
    </kbd>
  );
}

export default function HelpPage() {
  const scrollTo = (id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  };

  return (
    <div className="flex h-full overflow-hidden">
      <aside className="hidden lg:flex flex-col w-48 shrink-0 border-r border-border py-6 px-3 gap-0.5 overflow-y-auto">
        <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground px-2 mb-2">Contents</p>
        {sections.map(({ id, icon: Icon, title }) => (
          <button
            key={id}
            onClick={() => scrollTo(id)}
            className="flex items-center gap-2 px-2 py-1.5 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-muted/60 text-left transition-colors duration-100"
          >
            <Icon className="h-3.5 w-3.5 shrink-0" />
            {title}
          </button>
        ))}
      </aside>

      <main className="flex-1 overflow-y-auto px-8 py-8 max-w-3xl">
        <h1 className="text-2xl font-bold text-foreground mb-1">MediaHarbor Help</h1>
        <p className="text-muted-foreground text-sm mb-2">
          Everything you need to search, stream, and download music from 7 platforms.
        </p>

        <SectionAnchor id="faq" />
        <H2 icon={HelpCircle}>FAQ</H2>
        <div className="space-y-3">

          <FaqGroup title="Credentials & Sign-in" icon={Settings}>
            <FaqItem question="Unable to Download: Invalid Credentials">
              <p>
                MediaHarbor requires you to sign in with your own account to download from
                platforms other than YouTube.
              </p>
              <p className="font-medium text-foreground mt-2">Apple Music &amp; Spotify</p>
              <p>
                Install the <strong className="text-foreground">cookies.txt</strong> extension in
                your browser. Navigate to the Apple Music or Spotify page, export your cookies,
                then paste the file path (or contents) into{' '}
                <strong className="text-foreground">Settings → Credentials</strong>.
              </p>
              <p className="font-medium text-foreground mt-2">Deezer</p>
              <p>
                Open the Deezer player in your browser, open DevTools → Application → Cookies,
                find the <code className="font-mono bg-muted px-1 rounded text-xs">arl</code>{' '}
                cookie, copy its value, and paste it into Settings.
              </p>
              <p className="font-medium text-foreground mt-2">Qobuz</p>
              <p>
                Sign in with your email and password, <em>or</em> provide your User ID and
                app token in Settings.
              </p>
            </FaqItem>

            <FaqItem question="Do I need a paid subscription?">
              <p>
                Yes for lossless quality. Free-tier Spotify, Deezer, and Tidal accounts can
                only download lower-quality streams. Qobuz and Apple Music require an active
                paid subscription for Hi-Res downloads.
              </p>
            </FaqItem>
          </FaqGroup>

          <FaqGroup title="Downloads" icon={Download}>
            <FaqItem question="Downloads get stuck, what do I do?">
              <p>
                Open your task manager to check if your download process is still running. If it is not using the network, kill the process, then go to logs to copy the error output and
                open a new issue on{' '}
                <strong className="text-foreground">GitHub Issues</strong> with your logs
                attached.
              </p>
            </FaqItem>

            <FaqItem question="Deleted files still appear in the download list">
              <p>
                The downloads list tracks history, not file presence. Clearing individual
                entries from the list removes them from the history. A future update will
                automatically clean up entries whose files no longer exist.
              </p>
            </FaqItem>

            <FaqItem question="Which video sites can I download from?">
              <p>
                MediaHarbor uses <strong className="text-foreground">yt-dlp</strong> for
                generic video downloads. yt-dlp supports over 1,000 sites including YouTube,
                Vimeo, Twitch clips, Twitter/X, Instagram, and many more. Check the full list
                at{' '}
                <strong className="text-foreground">
                  github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md
                </strong>
                .
              </p>
            </FaqItem>
          </FaqGroup>

          <FaqGroup title="Project" icon={Heart}>
            <FaqItem question="How do I report a bug or request a feature?">
              <p>
                Visit{' '}
                <strong className="text-foreground">github.com/MediaHarbor/mediaharbor/issues</strong>{' '}
                and open a new issue. Use the Bug Report template for bugs and the Feature
                Request template for ideas.
              </p>
            </FaqItem>

            <FaqItem question="How can I support the project?">
              <p>
                Use the <strong className="text-foreground">Sponsor</strong> button on the
                GitHub project page. All contributions are appreciated and help keep the
                project active!
              </p>
            </FaqItem>

            <FaqItem question="How can I contribute code?">
              <p>
                Fork the repository, make your changes on a new branch, and open a Pull
                Request. Check existing issues labeled{' '}
                <code className="font-mono bg-muted px-1 rounded text-xs">good first issue</code>{' '}
                if you&apos;re looking for somewhere to start.
              </p>
            </FaqItem>
          </FaqGroup>
        </div>

        <SectionAnchor id="search" />
        <H2 icon={Search}>Search</H2>
        <div className="space-y-4 text-sm text-muted-foreground">
          <div>
            <p className="font-medium text-foreground mb-1">Typing hints</p>
            <p>
              As you type, MediaHarbor fetches music-aware suggestions from the iTunes catalog.
              Suggestions are grouped by <em>Artist</em>, <em>Track</em>, and <em>Album</em>.
              Use <Kbd>↑</Kbd> <Kbd>↓</Kbd> to navigate and <Kbd>Enter</Kbd> to pick one,
              or press <Kbd>Esc</Kbd> to dismiss.
            </p>
          </div>
          <div>
            <p className="font-medium text-foreground mb-1">Content types</p>
            <p>
              Switch between <strong className="text-foreground">Tracks</strong>,{' '}
              <strong className="text-foreground">Albums</strong>,{' '}
              <strong className="text-foreground">Playlists</strong>, and{' '}
              <strong className="text-foreground">Artists</strong> using the type chips next to
              the platform chips. Not all platforms support all types.
            </p>
          </div>
          <div>
            <p className="font-medium text-foreground mb-1">Opening albums &amp; playlists</p>
            <p>
              When searching for Albums or Playlists, click any result card to open its track
              listing. From there you can play individual tracks, play all tracks as a queue,
              or download the entire collection.
            </p>
          </div>
          <div className="flex items-start gap-2 rounded-lg bg-muted/40 border border-border px-4 py-3">
            <Disc3 className="h-4 w-4 mt-0.5 shrink-0 text-primary" />
            <span>
              Playing a track from search results automatically enqueues <em>all</em> current
              results, so playback continues to the next song when the current one ends.
            </span>
          </div>
        </div>

        <SectionAnchor id="downloads" />
        <H2 icon={Download}>Downloads</H2>
        <div className="space-y-3 text-sm text-muted-foreground">
          <p>
            Click the <strong className="text-foreground">Download</strong> icon on any result card, or
            paste a URL directly in the <strong className="text-foreground">Downloads</strong> page.
            A quality selector will appear before the download begins.
          </p>
          <div className="grid grid-cols-2 gap-2">
            {[
              ['FLAC / FLAC 24-bit', 'Lossless. Requires Tidal HiFi, Qobuz, or Deezer HQ subscription.'],
              ['AAC 256 kbps', 'High quality lossy from Apple Music.'],
              ['MP3 320 kbps', 'Standard lossy. Widely compatible.'],
              ['OGG Vorbis', 'Spotify streams are delivered as OGG.'],
            ].map(([q, d]) => (
              <div key={q} className="rounded-lg border border-border bg-card p-3">
                <p className="font-mono text-xs font-medium text-foreground mb-0.5">{q}</p>
                <p className="text-muted-foreground text-xs">{d}</p>
              </div>
            ))}
          </div>
          <Tip>Not all download formats are available for every track. Try another format if that one does not work.</Tip>
        </div>

        <SectionAnchor id="library" />
        <H2 icon={Library}>Library</H2>
        <div className="space-y-3 text-sm text-muted-foreground">
          <p>
            The Library page scans a folder on your disk and displays all audio files it finds.
            Click <strong className="text-foreground">Rescan</strong> to force refresh after new
            downloads. Library watches for new files everytime you download something.
          </p>
          <Tip>Set your download output folder in Settings → Downloads so that newly downloaded files appear in your library after the next scan.</Tip>
          <p>Supported formats include FLAC, MP3, AAC, OGG, WAV, AIFF, and M4A.</p>
        </div>

        <SectionAnchor id="shortcuts" />
        <H2 icon={Keyboard}>Keyboard Shortcuts</H2>
        <div className="rounded-lg border border-border bg-card divide-y divide-border text-sm">
          {([
            [[<Kbd key="sp">Space</Kbd>],                              'Toggle play / pause (only when not focused on a text input)'],
            [[<Kbd key="u">↑</Kbd>, ' ', <Kbd key="d">↓</Kbd>],       'Navigate autocomplete suggestions in the search bar'],
            [[<Kbd key="en">Enter</Kbd>],                              'Accept highlighted suggestion or submit search'],
            [[<Kbd key="es">Esc</Kbd>],                                'Dismiss autocomplete suggestion dropdown'],
          ] as const).map(([key, desc], i) => (
            <div key={i} className="flex items-center gap-4 px-3 py-2.5">
              <span className="flex gap-1 items-center w-28 shrink-0">{key}</span>
              <span className="text-muted-foreground">{desc}</span>
            </div>
          ))}
          <Tip>More customizable shortcuts will be added on next updates</Tip>
        </div>

        <div className="mt-10 mb-4 flex items-center justify-center gap-4 text-xs text-muted-foreground">
          <button
            type="button"
            onClick={() => window.electron?.updates.openRelease('https://github.com/MediaHarbor/mediaharbor/issues')}
            className="flex items-center gap-1.5 hover:text-foreground transition-colors"
          >
            <GitFork className="h-3.5 w-3.5" />
            Report an issue
          </button>
          <span>·</span>
          <button
            type="button"
            onClick={() => window.electron?.updates.openRelease('https://github.com/MediaHarbor/mediaharbor')}
            className="flex items-center gap-1.5 hover:text-foreground transition-colors"
          >
            <Heart className="h-3.5 w-3.5" />
            Sponsor
          </button>
        </div>
      </main>
    </div>
  );
}
