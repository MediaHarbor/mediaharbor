import { ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { Player } from '@/components/Player';

interface MainLayoutProps {
  children: ReactNode;
}

export function MainLayout({ children }: MainLayoutProps) {
  return (
    <div className="h-screen flex flex-col overflow-hidden bg-background">
      <div className="flex flex-1 min-h-0 overflow-hidden">
        <Sidebar />
        {children}
      </div>
      <Player />
    </div>
  );
}
