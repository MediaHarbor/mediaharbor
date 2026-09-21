import {
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
} from '@/components/ui/dropdown-menu';
import {
  ContextMenuItem,
  ContextMenuCheckboxItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubTrigger,
  ContextMenuSubContent,
} from '@/components/ui/context-menu';
import { cn } from '@/utils/cn';
import type { ActionItem } from './types';

export type MenuType = 'dropdown' | 'context';

interface ActionItemsProps {
  items: ActionItem[];
  menuType: MenuType;
}

export function ActionItems({ items, menuType }: ActionItemsProps) {
  return (
    <>
      {items
        .filter((it) => !it.hidden)
        .map((it) => (
          <RenderedItem key={it.id} item={it} menuType={menuType} />
        ))}
    </>
  );
}

function RenderedItem({ item, menuType }: { item: ActionItem; menuType: MenuType }) {
  if (item.separator) {
    const Sep = menuType === 'dropdown' ? DropdownMenuSeparator : ContextMenuSeparator;
    return <Sep />;
  }
  if (item.submenu) {
    return <SubMenu item={item} menuType={menuType} />;
  }
  if (typeof item.checked === 'boolean') {
    const Check = menuType === 'dropdown' ? DropdownMenuCheckboxItem : ContextMenuCheckboxItem;
    return (
      <Check
        checked={item.checked}
        disabled={item.disabled}
        onCheckedChange={() => item.action?.()}
        className={cn(item.destructive && 'text-destructive focus:text-destructive')}
      >
        <span className="flex items-center gap-2">
          {item.icon}
          {item.label}
        </span>
      </Check>
    );
  }
  const Item = menuType === 'dropdown' ? DropdownMenuItem : ContextMenuItem;
  return (
    <Item
      disabled={item.disabled}
      onSelect={() => item.action?.()}
      className={cn(item.destructive && 'text-destructive focus:text-destructive')}
    >
      {item.icon}
      <span>{item.label}</span>
    </Item>
  );
}

function SubMenu({ item, menuType }: { item: ActionItem; menuType: MenuType }) {
  if (menuType === 'dropdown') {
    return (
      <DropdownMenuSub>
        <DropdownMenuSubTrigger disabled={item.disabled}>
          <span className="flex items-center gap-2">
            {item.icon}
            {item.label}
          </span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <ActionItems items={item.submenu ?? []} menuType="dropdown" />
        </DropdownMenuSubContent>
      </DropdownMenuSub>
    );
  }
  return (
    <ContextMenuSub>
      <ContextMenuSubTrigger disabled={item.disabled}>
        <span className="flex items-center gap-2">
          {item.icon}
          {item.label}
        </span>
      </ContextMenuSubTrigger>
      <ContextMenuSubContent>
        <ActionItems items={item.submenu ?? []} menuType="context" />
      </ContextMenuSubContent>
    </ContextMenuSub>
  );
}

export { DropdownMenuLabel, ContextMenuLabel };
