import { useState, useCallback } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import {
  Dialog,
  DialogContent,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { ChevronLeft, ChevronRight, Compass } from 'lucide-react';
import { useOnboardingStore } from '../stores/useOnboardingStore';
import { OnboardingStepIndicator } from './OnboardingStepIndicator';
import { StepWelcome } from './StepWelcome';
import { StepInstallIntro } from './StepInstallIntro';
import { StepInstallSequence } from './StepInstallSequence';
import { StepBasicSetup } from './StepBasicSetup';
import { StepReadyForTour } from './StepReadyForTour';

const slideVariants = {
  enter: (dir: number) => ({ x: dir > 0 ? 60 : -60, opacity: 0 }),
  center: { x: 0, opacity: 1 },
  exit: (dir: number) => ({ x: dir > 0 ? -60 : 60, opacity: 0 }),
};

const TOTAL_STEPS = 5;

export function OnboardingWizard() {
  const { isOpen, currentStep, direction, nextStep, prevStep, finishWizard } =
    useOnboardingStore();
  const [isInstalling, setIsInstalling] = useState(false);
  const [installAllDone, setInstallAllDone] = useState(false);

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open) void finishWizard({ startTour: false });
    },
    [finishWizard]
  );

  const handleSkip = () => void finishWizard({ startTour: false });
  const handleStartTour = () => void finishWizard({ startTour: true });

  if (!isOpen) return null;

  const stepComponents = [
    <StepWelcome key="welcome" />,
    <StepInstallIntro key="install-intro" />,
    <StepInstallSequence
      key="install-seq"
      onInstallingChange={setIsInstalling}
      onAllDone={() => setInstallAllDone(true)}
    />,
    <StepBasicSetup key="setup" />,
    <StepReadyForTour key="ready" />,
  ];

  const isInstallSequenceStep = currentStep === 2;
  const installSequenceBlocked = isInstallSequenceStep && !installAllDone;
  const isFinalStep = currentStep === TOTAL_STEPS - 1;

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-2xl p-0 gap-0 overflow-hidden sm:rounded-2xl [&>button]:hidden">
        <OnboardingStepIndicator currentStep={currentStep} />
        <div className="relative overflow-hidden" style={{ minHeight: 380 }}>
          <AnimatePresence mode="wait" initial={false} custom={direction}>
            <motion.div
              key={currentStep}
              custom={direction}
              variants={slideVariants}
              initial="enter"
              animate="center"
              exit="exit"
              transition={{ type: 'spring', stiffness: 300, damping: 30 }}
              className="absolute inset-0 p-6 overflow-y-auto"
            >
              {stepComponents[currentStep]}
            </motion.div>
          </AnimatePresence>
        </div>
        <DialogFooter className="border-t border-border/40 px-6 py-4 flex-row items-center justify-between sm:justify-between">
          <div className="flex items-center gap-2">
            {currentStep > 0 && !isInstalling && !isInstallSequenceStep && (
              <Button variant="ghost" size="sm" onClick={prevStep} className="gap-1">
                <ChevronLeft className="w-4 h-4" />
                Back
              </Button>
            )}
          </div>
          <div className="flex items-center gap-2">
            {!isFinalStep && !isInstalling && (
              <Button variant="ghost" size="sm" onClick={handleSkip}>
                Skip setup
              </Button>
            )}
            {isFinalStep ? (
              <Button size="sm" onClick={handleStartTour} className="gap-1">
                <Compass className="w-4 h-4" />
                Start tour
              </Button>
            ) : (
              <Button
                size="sm"
                onClick={nextStep}
                disabled={isInstalling || installSequenceBlocked}
                className="gap-1"
              >
                {currentStep === 0 ? 'Get Started' : 'Next'}
                <ChevronRight className="w-4 h-4" />
              </Button>
            )}
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
