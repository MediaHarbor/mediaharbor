import { ReactNode } from 'react';
import { AnimatePresence } from 'framer-motion';
import { Sidebar } from './Sidebar';
import { Player } from '@/components/Player';
import { NowPlaying } from '@/components/NowPlaying';
import { usePlayerStore } from '@/stores/usePlayerStore';

interface MainLayoutProps {
  children: ReactNode;
}

export function MainLayout({ children }: MainLayoutProps) {
  const nowPlayingOpen = usePlayerStore((s) => s.nowPlayingOpen);

  return (
    <div className="h-screen flex flex-col overflow-hidden bg-background">
      <div className="flex flex-1 min-h-0 overflow-hidden relative">
        <Sidebar />
        <div className="flex flex-1 min-w-0 overflow-hidden">
          {children}
        </div>
      </div>
      <Player />
      <AnimatePresence>
        {nowPlayingOpen && <NowPlaying />}
      </AnimatePresence>
    </div>
  );
}
