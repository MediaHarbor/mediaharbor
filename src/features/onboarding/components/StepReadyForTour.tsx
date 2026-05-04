import { motion } from 'framer-motion';
import { Compass } from 'lucide-react';

export function StepReadyForTour() {
  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      className="flex flex-col items-center text-center gap-5 py-6"
    >
      <div className="rounded-2xl bg-primary/10 p-4">
        <Compass className="w-7 h-7 text-primary" />
      </div>
      <div className="space-y-2 max-w-sm">
        <h2 className="text-lg font-semibold">Ready for the grand tour?</h2>
        <p className="text-sm text-muted-foreground">
          I&apos;ll close this dialog and walk you through the actual app — pointing out where each
          section lives in the sidebar so you know where to find things.
        </p>
      </div>
      <p className="text-xs text-muted-foreground">
        You can skip the tour anytime from the popover.
      </p>
    </motion.div>
  );
}
