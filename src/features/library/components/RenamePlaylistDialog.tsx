import { useEffect, useState } from 'react';
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
import { useRenameServicePlaylist } from '@/features/library/hooks/useLibraryMutations';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  platform: string;
  playlistId: string;
  currentName: string;
  currentDescription?: string;
  currentIsPublic?: boolean;
  currentIsCollaborative?: boolean;
  supportsVisibility?: boolean;
}

export function RenamePlaylistDialog({
  open,
  onOpenChange,
  platform,
  playlistId,
  currentName,
  currentDescription,
  currentIsPublic,
  currentIsCollaborative,
  supportsVisibility = true,
}: Props) {
  const [name, setName] = useState(currentName);
  const [description, setDescription] = useState(currentDescription ?? '');
  const [isPublic, setIsPublic] = useState(!!currentIsPublic);
  const [isCollaborative, setIsCollaborative] = useState(!!currentIsCollaborative);
  const rename = useRenameServicePlaylist();

  useEffect(() => {
    if (open) {
      setName(currentName);
      setDescription(currentDescription ?? '');
      setIsPublic(!!currentIsPublic);
      setIsCollaborative(!!currentIsCollaborative);
    }
  }, [open, currentName, currentDescription, currentIsPublic, currentIsCollaborative]);

  const submit = async () => {
    if (!name.trim()) return;
    await rename.mutateAsync({
      platform,
      id: playlistId,
      name: name.trim(),
      description: description.trim() || undefined,
      isPublic: supportsVisibility ? isPublic : undefined,
      isCollaborative: supportsVisibility ? isCollaborative : undefined,
    });
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit playlist</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <Input
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
          {supportsVisibility && (
            <>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox checked={isPublic} onCheckedChange={(v) => setIsPublic(v === true)} />
                <span>Public</span>
              </label>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={isCollaborative}
                  onCheckedChange={(v) => setIsCollaborative(v === true)}
                />
                <span>Collaborative (others can add tracks)</span>
              </label>
            </>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={!name.trim() || rename.isPending}>
            {rename.isPending ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
