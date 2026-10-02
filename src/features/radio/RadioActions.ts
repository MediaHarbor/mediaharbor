import { createContext, useContext } from 'react';
import type { RadioList, RadioStation } from '@/features/radio/api';

/**
 * What a station card can do, and the playlists it can be added to.
 *
 * Provided once by the radio page rather than threaded through every view. It
 * was a nine-prop bag drilled four levels deep — page → lists view → list detail
 * → row — which meant adding one action touched eight files.
 *
 * Only the stable half lives here. `favoriteKeys` and `nowPlayingKey` change on
 * every toggle and every track change, so they stay props: as context they would
 * re-render every consumer where `memo` can currently skip.
 */
export interface RadioActions {
  onPlay: (station: RadioStation) => void;
  onToggleFavorite: (station: RadioStation, favorite: boolean) => void;
  onAddToList: (station: RadioStation, list: RadioList) => void;
  onCreateListWith: (station: RadioStation) => void;
  onEdit: (station: RadioStation) => void;
  onForget: (station: RadioStation) => void;
  /** The playlists the "add to playlist" submenu offers. */
  lists: RadioList[];
}

export const RadioActionsContext = createContext<RadioActions | null>(null);

export function useRadioActions(): RadioActions {
  const actions = useContext(RadioActionsContext);
  if (!actions) throw new Error('useRadioActions must be used inside RadioActionsContext');
  return actions;
}

/** Which stations are favourites, and which one is playing. */
export interface StationChrome {
  favoriteKeys: ReadonlySet<string>;
  nowPlayingKey: string | null;
}
