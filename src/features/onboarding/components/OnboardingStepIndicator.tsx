import { Check } from 'lucide-react';
import { cn } from '@/utils/cn';

const STEP_LABELS = ['Welcome', 'Install', 'Tools', 'Setup', 'Tour'];

interface OnboardingStepIndicatorProps {
  currentStep: number;
}

export function OnboardingStepIndicator({ currentStep }: OnboardingStepIndicatorProps) {
  return (
    <div className="flex items-center justify-center px-6 py-4 border-b border-border/30">
      {STEP_LABELS.map((label, i) => (
        <div key={i} className="flex items-center">
          <div className="flex flex-col items-center gap-1">
            <div
              className={cn(
                'flex items-center justify-center w-7 h-7 rounded-full text-xs font-semibold transition-colors',
                i < currentStep && 'bg-primary/40 text-primary-foreground',
                i === currentStep && 'bg-primary text-primary-foreground',
                i > currentStep && 'bg-muted/40 border border-border text-muted-foreground'
              )}
            >
              {i < currentStep ? <Check className="w-3.5 h-3.5" /> : <span>{i + 1}</span>}
            </div>
            <span
              className={cn(
                'text-[10px] font-medium',
                i === currentStep ? 'text-foreground' : 'text-muted-foreground'
              )}
            >
              {label}
            </span>
          </div>
          {i < STEP_LABELS.length - 1 && (
            <div
              className={cn(
                'h-px w-10 mb-4 mx-1 transition-colors',
                i < currentStep ? 'bg-primary' : 'bg-border'
              )}
            />
          )}
        </div>
      ))}
    </div>
  );
}
