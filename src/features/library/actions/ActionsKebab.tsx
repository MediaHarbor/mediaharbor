import { MoreHorizontal } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/utils/cn';
import { ActionItems } from './ActionItems';
import type { ActionItem } from './types';

interface ActionsKebabProps {
  items: ActionItem[];
  className?: string;
  size?: 'sm' | 'md';
  align?: 'start' | 'center' | 'end';
}

export function ActionsKebab({ items, className, size = 'md', align = 'end' }: ActionsKebabProps) {
  const hasItems = items.some((it) => !it.hidden && !it.separator);
  if (!hasItems) return null;

  const dim = size === 'sm' ? 'h-7 w-7' : 'h-8 w-8';
  const iconCls = size === 'sm' ? 'h-3.5 w-3.5' : 'h-4 w-4';

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label="More actions"
          title="More"
          onClick={(e) => e.stopPropagation()}
          className={cn(
            'flex items-center justify-center rounded-md text-muted-foreground',
            'hover:text-foreground hover:bg-accent transition-colors duration-100',
            dim,
            className
          )}
        >
          <MoreHorizontal className={iconCls} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align={align} className="w-56" onClick={(e) => e.stopPropagation()}>
        <ActionItems items={items} menuType="dropdown" />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
