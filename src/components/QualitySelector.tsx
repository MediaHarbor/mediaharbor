import { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { Platform } from '@/types';
import { QUALITY_OPTIONS } from '@/utils/constants';

interface QualitySelectorProps {
  open: boolean;
  onClose: () => void;
  platform: Platform;
  title: string;
  onConfirm: (quality: string) => void;
}

export function QualitySelector({
  open,
  onClose,
  platform,
  title,
  onConfirm,
}: QualitySelectorProps) {
  const options = QUALITY_OPTIONS[platform] || [];
  const [selectedQuality, setSelectedQuality] = useState(options[0]?.value || '');

  const handleConfirm = () => {
    onConfirm(selectedQuality);
    onClose();
  };

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Select Quality</DialogTitle>
          <DialogDescription>
            Choose download quality for &quot;{title}&quot;
          </DialogDescription>
        </DialogHeader>

        <div className="py-4">
          <Select value={selectedQuality} onValueChange={setSelectedQuality}>
            <SelectTrigger>
              <SelectValue placeholder="Select quality" />
            </SelectTrigger>
            <SelectContent>
              {options.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <DialogFooter className="flex gap-2">
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={handleConfirm}>
            Download
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
