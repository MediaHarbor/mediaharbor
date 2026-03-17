import { useState } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { Search, Download, Library, Settings, HelpCircle, RefreshCw, ScrollText, PanelLeftClose, PanelLeft } from 'lucide-react';
import { motion } from 'framer-motion';
import { cn } from '@/utils/cn';
import logoUrl from '@/assets/MediaHarbor_Logo.svg';

const TOP_NAV = [
  { path: '/search',    icon: Search,   label: 'Search' },
  { path: '/downloads', icon: Download, label: 'Downloads' },
  { path: '/library',   icon: Library,  label: 'Library' },
];

const BOTTOM_NAV = [
  { path: '/updates',  icon: RefreshCw,  label: 'Updates' },
  { path: '/logs',     icon: ScrollText, label: 'Logs' },
  { path: '/help',     icon: HelpCircle, label: 'Help' },
  { path: '/settings', icon: Settings,   label: 'Settings' },
];

function NavItem({ path, icon: Icon, label, isActive, collapsed }: {
  path: string;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  isActive: boolean;
  collapsed: boolean;
}) {
  return (
    <Link
      key={path}
      to={path}
      title={collapsed ? label : undefined}
      className={cn(
        'relative flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors duration-150',
        collapsed && 'justify-center px-0',
        isActive
          ? 'text-accent-foreground font-medium'
          : 'text-muted-foreground hover:text-foreground hover:bg-muted/60'
      )}
    >
      {isActive && (
        <motion.div
          layoutId="sidebar-active"
          className="absolute inset-0 rounded-lg bg-accent"
          transition={{ type: 'spring', bounce: 0.15, duration: 0.35 }}
        />
      )}
      <Icon className="relative z-10 h-4 w-4 shrink-0" />
      {!collapsed && <span className="relative z-10">{label}</span>}
    </Link>
  );
}

export function Sidebar() {
  const location = useLocation();
  const [collapsed, setCollapsed] = useState(false);

  const isActive = (path: string) =>
    location.pathname === path || (path === '/search' && location.pathname === '/');

  return (
    <aside
      className={cn(
        'shrink-0 flex flex-col border-r border-border bg-card/40 select-none transition-all duration-200',
        collapsed ? 'w-14' : 'w-56'
      )}
    >
      {/* Header */}
      <div className={cn(
        'flex items-center shrink-0 border-b border-border/40',
        collapsed ? 'justify-center py-3 px-1.5' : 'h-14 px-3 gap-2.5'
      )}>
        {!collapsed && (
          <>
            <img src={logoUrl} alt="MediaHarbor" className="h-7 w-7 rounded-lg shrink-0" />
            <span className="font-semibold text-sm tracking-tight flex-1">MediaHarbor</span>
          </>
        )}
        <button
          onClick={() => setCollapsed(c => !c)}
          className={cn(
            'rounded-md text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors shrink-0',
            collapsed ? 'p-2' : 'p-1'
          )}
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? <PanelLeft className="h-4 w-4" /> : <PanelLeftClose className="h-4 w-4" />}
        </button>
      </div>

      {/* Top nav */}
      <nav className={cn('py-3 space-y-0.5', collapsed ? 'px-1.5' : 'px-2')}>
        {TOP_NAV.map(item => (
          <NavItem key={item.path} {...item} isActive={isActive(item.path)} collapsed={collapsed} />
        ))}
      </nav>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Bottom nav */}
      <nav className={cn('border-t border-border/40 py-3 space-y-0.5', collapsed ? 'px-1.5' : 'px-2')}>
        {BOTTOM_NAV.map(item => (
          <NavItem key={item.path} {...item} isActive={isActive(item.path)} collapsed={collapsed} />
        ))}
      </nav>
    </aside>
  );
}
