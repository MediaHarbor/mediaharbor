import { useState, useEffect } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectTrigger, SelectValue } from '@/components/ui/select';
import { QualityOptionItems } from '@/components/QualityOptionItems';
import { Slider } from '@/components/ui/slider';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import type { Platform } from '@/types';
import { SKIP_AWARE_PLATFORMS } from '@/utils/constants';
import { ytMusicSliderQuality } from '@/utils/quality';
import { useQualityOptions } from '@/hooks/useQualityOptions';

interface QualitySelectorProps {
  open: boolean;
  onClose: () => void;
  platform: Platform;
  title: string;
  onConfirm: (quality: string, forceRedownload: boolean) => void;
}

export function QualitySelector({
  open,
  onClose,
  platform,
  title,
  onConfirm,
}: QualitySelectorProps) {
  const { options, defaultQuality } = useQualityOptions(platform, open);

  const [userSelected, setUserSelected] = useState('');
  const [sliderValue, setSliderValue] = useState(10);
  const [forceRedownload, setForceRedownload] = useState(false);
  const selectedQuality = options.find((o) => o.value === userSelected)?.value ?? defaultQuality;

  useEffect(() => {
    if (open) {
      setForceRedownload(false);
      setUserSelected('');
    }
  }, [open]);

  const supportsSkip = SKIP_AWARE_PLATFORMS.has(platform);

  const handleConfirm = () => {
    if (platform === 'youtubemusic') {
      onConfirm(ytMusicSliderQuality(sliderValue), forceRedownload);
    } else {
      onConfirm(selectedQuality, forceRedownload);
    }
    onClose();
  };

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Select Quality</DialogTitle>
          <DialogDescription>Choose download quality for &quot;{title}&quot;</DialogDescription>
        </DialogHeader>

        <div className="py-4">
          {platform === 'youtubemusic' ? (
            <div className="space-y-4">
              <div className="text-center text-sm font-medium">{sliderValue}</div>
              <Slider
                min={0}
                max={10}
                step={1}
                value={[sliderValue]}
                onValueChange={([v]) => setSliderValue(v)}
              />
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>0 — Worst</span>
                <span>10 — Best</span>
              </div>
            </div>
          ) : (
            <Select value={selectedQuality} onValueChange={setUserSelected}>
              <SelectTrigger>
                <SelectValue placeholder="Select quality" />
              </SelectTrigger>
              <SelectContent>
                <QualityOptionItems options={options} />
              </SelectContent>
            </Select>
          )}

          {supportsSkip && (
            <div className="mt-4 flex items-start gap-2">
              <Checkbox
                id="force-redownload"
                checked={forceRedownload}
                onCheckedChange={(v) => setForceRedownload(!!v)}
                className="mt-0.5"
              />
              <div>
                <Label htmlFor="force-redownload" className="font-normal cursor-pointer">
                  Force re-download
                </Label>
                <p className="text-xs text-muted-foreground">
                  Download even if this track was already downloaded from this service.
                </p>
              </div>
            </div>
          )}
        </div>

        <DialogFooter className="flex gap-2">
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={handleConfirm}>Download</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
