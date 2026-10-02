import type { ReactNode } from 'react';
import { Globe, Heart, ListPlus, Pencil, Play, Trash2 } from 'lucide-react';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import { tauriAPI } from '@/tauri-bridge';
import { useRadioActions } from '@/features/radio/RadioActions';
import type { RadioStation } from '@/features/radio/api';

interface Props {
  station: RadioStation;
  favorite: boolean;
  /** Only a station inside a playlist can be taken out of one. */
  onRemoveFromList?: (station: RadioStation) => void;
  /** The card or row the menu is attached to. */
  children: ReactNode;
}

/**
 * The right-click menu a station carries, wherever it is drawn.
 *
 * Cards and rows had a copy each. They had already drifted — the same action was
 * "Add to list" on one and "Add to playlist" on the other — which is exactly the
 * failure a second copy invites.
 */
export function StationMenu({ station, favorite, onRemoveFromList, children }: Props) {
  const { onPlay, onToggleFavorite, onAddToList, onCreateListWith, onEdit, onForget, lists } =
    useRadioActions();

  // Only the user's own stations can be deleted. Un-favouriting is what a
  // directory station gets; deleting its cached row would look the same and do
  // nothing, since the next search puts it straight back.
  const deletable = station.source === 'custom';

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>

      <ContextMenuContent className="w-56">
        <ContextMenuItem onSelect={() => onPlay(station)}>
          <Play className="mr-2 h-3.5 w-3.5" /> Play
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onToggleFavorite(station, !favorite)}>
          <Heart className="mr-2 h-3.5 w-3.5" />
          {favorite ? 'Remove from favourites' : 'Save station'}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onEdit(station)}>
          <Pencil className="mr-2 h-3.5 w-3.5" /> Edit station…
        </ContextMenuItem>
        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <ListPlus className="mr-2 h-3.5 w-3.5" /> Add to playlist
          </ContextMenuSubTrigger>
          <ContextMenuSubContent className="max-h-64 w-52 overflow-y-auto">
            <ContextMenuItem onSelect={() => onCreateListWith(station)}>
              New playlist…
            </ContextMenuItem>
            {lists.length > 0 && <ContextMenuSeparator />}
            {lists.map((list) => (
              <ContextMenuItem key={list.id} onSelect={() => onAddToList(station, list)}>
                <span className="truncate">{list.name}</span>
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>
        {station.homepage && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem
              onSelect={() => void tauriAPI.app.openExternal(station.homepage as string)}
            >
              <Globe className="mr-2 h-3.5 w-3.5" /> Open website
            </ContextMenuItem>
          </>
        )}
        {(onRemoveFromList || deletable) && <ContextMenuSeparator />}
        {onRemoveFromList && (
          <ContextMenuItem onSelect={() => onRemoveFromList(station)}>
            <Trash2 className="mr-2 h-3.5 w-3.5" /> Remove from playlist
          </ContextMenuItem>
        )}
        {deletable && (
          <ContextMenuItem className="text-destructive" onSelect={() => onForget(station)}>
            <Trash2 className="mr-2 h-3.5 w-3.5" /> Delete station
          </ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}
