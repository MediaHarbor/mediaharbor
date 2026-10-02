import { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { useCreateServicePlaylist } from '@/features/library/hooks/useLibraryMutations';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  platform: string;
  initialTrackIds?: string[];
  onCreated?: (playlistId: string) => void;
}

export function CreatePlaylistDialog({
  open,
  onOpenChange,
  platform,
  initialTrackIds,
  onCreated,
}: Props) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [isPublic, setIsPublic] = useState(false);
  const [isCollaborative, setIsCollaborative] = useState(false);
  const create = useCreateServicePlaylist();

  const submit = async () => {
    if (!name.trim()) return;
    const res = await create.mutateAsync({
      platform,
      name: name.trim(),
      description: description.trim() || undefined,
      isPublic,
      isCollaborative,
      initialTrackIds,
    });
    onCreated?.(res.playlist_id);
    setName('');
    setDescription('');
    setIsPublic(false);
    setIsCollaborative(false);
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New playlist</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <Input
            placeholder="Playlist name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter') void submit();
            }}
          />
          <Input
            placeholder="Description (optional)"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={isPublic} onCheckedChange={(v) => setIsPublic(v === true)} />
            <span>Make playlist public</span>
          </label>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={isCollaborative}
              onCheckedChange={(v) => setIsCollaborative(v === true)}
            />
            <span>Collaborative (others can add tracks)</span>
          </label>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={!name.trim() || create.isPending}>
            {create.isPending ? 'Creating…' : 'Create'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
