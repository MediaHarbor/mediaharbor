import { Play, ListEnd, ListPlus, FolderOpen, Copy, Info, Radio } from 'lucide-react';
import type { PlayableTrack } from '@/stores/usePlayerStore';
import type { ActionItem } from './types';
import { copyText } from '@/utils/clipboard';
import { tauriAPI } from '@/tauri-bridge';

const icon = 'h-4 w-4';

export function buildLocalTrackItems(opts: {
  playable: PlayableTrack;
  path: string;
  onPlay: () => void;
  insertNext: (t: PlayableTrack) => void;
  appendToQueue: (t: PlayableTrack[]) => void;
  onShowInFolder?: () => void;
  onShowInfo?: () => void;
  onStartRadio?: () => void;
}): ActionItem[] {
  const { playable, path, onPlay, insertNext, appendToQueue } = opts;
  const showInFolder =
    opts.onShowInFolder ??
    (() => {
      void tauriAPI.downloads.showItemInFolder(path);
    });
  return [
    { id: 'play', label: 'Play', icon: <Play className={icon} />, action: onPlay },
    {
      id: 'play-next',
      label: 'Play next',
      icon: <ListEnd className={icon} />,
      action: () => insertNext(playable),
    },
    {
      id: 'add-queue',
      label: 'Add to queue',
      icon: <ListPlus className={icon} />,
      action: () => appendToQueue([playable]),
    },
    ...(opts.onStartRadio
      ? [
          {
            id: 'radio',
            label: 'Start radio',
            icon: <Radio className={icon} />,
            action: opts.onStartRadio,
          } satisfies ActionItem,
        ]
      : []),
    { id: 'sep', label: '', separator: true },
    ...(opts.onShowInfo
      ? [
          {
            id: 'info',
            label: 'Get info',
            icon: <Info className={icon} />,
            action: opts.onShowInfo,
          } satisfies ActionItem,
        ]
      : []),
    {
      id: 'folder',
      label: 'Show in folder',
      icon: <FolderOpen className={icon} />,
      action: showInFolder,
    },
    {
      id: 'copy-path',
      label: 'Copy file path',
      icon: <Copy className={icon} />,
      action: () => {
        void copyText(path);
      },
    },
  ];
}
