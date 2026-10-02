import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useDeleteServicePlaylist } from '@/features/library/hooks/useLibraryMutations';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  platform: string;
  playlistId: string;
  playlistName: string;
}

export function DeletePlaylistConfirm({
  open,
  onOpenChange,
  platform,
  playlistId,
  playlistName,
}: Props) {
  const del = useDeleteServicePlaylist();
  const submit = async () => {
    await del.mutateAsync({ platform, id: playlistId });
    onOpenChange(false);
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete playlist?</DialogTitle>
          <DialogDescription>
            &ldquo;{playlistName}&rdquo; will be removed from your library. Other users who follow
            it keep their copy.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={submit} disabled={del.isPending}>
            {del.isPending ? 'Deleting…' : 'Delete'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
