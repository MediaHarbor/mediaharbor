import { useEffect, useLayoutEffect, useState, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useNavigate, useLocation } from 'react-router-dom';
import { AnimatePresence, motion } from 'framer-motion';
import { ChevronLeft, ChevronRight, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useTourStore } from '../stores/useTourStore';
import { TOUR_STEPS, type TourStep } from '../tour/steps';

interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

const PADDING = 8;
const POPOVER_WIDTH = 320;
const POPOVER_GAP = 14;
const VIEWPORT_MARGIN = 12;

function measure(selector?: string): Rect | null {
  if (!selector) return null;
  const el = document.querySelector(selector) as HTMLElement | null;
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return {
    top: r.top - PADDING,
    left: r.left - PADDING,
    width: r.width + PADDING * 2,
    height: r.height + PADDING * 2,
  };
}

function popoverPosition(
  rect: Rect | null,
  placement: TourStep['placement'],
  popHeight: number,
  popWidth: number
): { top: number; left: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  const clamp = (top: number, left: number) => ({
    top: Math.max(VIEWPORT_MARGIN, Math.min(vh - popHeight - VIEWPORT_MARGIN, top)),
    left: Math.max(VIEWPORT_MARGIN, Math.min(vw - popWidth - VIEWPORT_MARGIN, left)),
  });

  if (!rect || placement === 'center') {
    return clamp((vh - popHeight) / 2, (vw - popWidth) / 2);
  }
  switch (placement) {
    case 'right':
      return clamp(rect.top + rect.height / 2 - popHeight / 2, rect.left + rect.width + POPOVER_GAP);
    case 'left':
      return clamp(rect.top + rect.height / 2 - popHeight / 2, rect.left - POPOVER_GAP - popWidth);
    case 'bottom':
      return clamp(rect.top + rect.height + POPOVER_GAP, rect.left + rect.width / 2 - popWidth / 2);
    case 'top':
    default:
      return clamp(rect.top - POPOVER_GAP - popHeight, rect.left + rect.width / 2 - popWidth / 2);
  }
}

export function SpotlightTour() {
  const { isOpen, currentStep, next, prev, end } = useTourStore();
  const step = TOUR_STEPS[currentStep];
  const navigate = useNavigate();
  const location = useLocation();
  const [rect, setRect] = useState<Rect | null>(null);
  const [viewportTick, setViewportTick] = useState(0);
  const [popSize, setPopSize] = useState({ width: POPOVER_WIDTH, height: 200 });
  const popoverRef = useRef<HTMLDivElement>(null);
  const lastNavigatedRoute = useRef<string | null>(null);

  useEffect(() => {
    if (!isOpen || !step) return;
    if (step.route && location.pathname !== step.route && lastNavigatedRoute.current !== step.route) {
      lastNavigatedRoute.current = step.route;
      navigate(step.route);
    }
  }, [isOpen, step, location.pathname, navigate]);

  useLayoutEffect(() => {
    if (!isOpen || !step) return;
    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      setRect(measure(step.selector));
      raf2 = requestAnimationFrame(() => setRect(measure(step.selector)));
    });
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
    };
  }, [isOpen, step, location.pathname, viewportTick]);

  useEffect(() => {
    if (!isOpen) return;
    const onResize = () => setViewportTick((t) => t + 1);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [isOpen]);

  useLayoutEffect(() => {
    if (!isOpen || !popoverRef.current) return;
    const r = popoverRef.current.getBoundingClientRect();
    setPopSize((prev) =>
      Math.abs(prev.height - r.height) < 1 && Math.abs(prev.width - r.width) < 1
        ? prev
        : { width: r.width, height: r.height }
    );
  }, [isOpen, step, rect]);

  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void end();
      else if (e.key === 'ArrowRight' || e.key === 'Enter') next();
      else if (e.key === 'ArrowLeft') prev();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen, next, prev, end]);

  if (!isOpen || !step) return null;

  const pos = popoverPosition(rect, step.placement, popSize.height, popSize.width);
  const totalSteps = TOUR_STEPS.length;
  const isLast = currentStep === totalSteps - 1;
  const isFirst = currentStep === 0;

  return createPortal(
    <div className="fixed inset-0 z-[100] pointer-events-none">
      {/* Spotlight backdrop */}
      <svg
        className="absolute inset-0 w-full h-full pointer-events-auto"
        onClick={() => next()}
      >
        <defs>
          <mask id="tour-mask">
            <rect width="100%" height="100%" fill="white" />
            {rect && (
              <motion.rect
                animate={{
                  x: rect.left,
                  y: rect.top,
                  width: rect.width,
                  height: rect.height,
                }}
                transition={{ type: 'spring', stiffness: 260, damping: 30 }}
                rx={10}
                ry={10}
                fill="black"
              />
            )}
          </mask>
        </defs>
        <rect
          width="100%"
          height="100%"
          fill="rgba(0,0,0,0.6)"
          mask={rect ? 'url(#tour-mask)' : undefined}
        />
      </svg>

      {/* Popover */}
      <AnimatePresence mode="wait">
        <motion.div
          key={step.id}
          ref={popoverRef}
          initial={{ opacity: 0, scale: 0.96 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: 0.96 }}
          transition={{ duration: 0.18 }}
          style={{
            position: 'fixed',
            top: pos.top,
            left: pos.left,
            width: POPOVER_WIDTH,
          }}
          className="pointer-events-auto rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl p-4 space-y-3"
        >
          <div className="flex items-start justify-between gap-2">
            <div className="space-y-1 flex-1">
              <p className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                Step {currentStep + 1} of {totalSteps}
              </p>
              <h3 className="text-sm font-semibold leading-tight">{step.title}</h3>
            </div>
            <button
              onClick={() => void end()}
              className="text-muted-foreground hover:text-foreground transition-colors p-0.5 -m-0.5"
              aria-label="Skip tour"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
          <p className="text-xs text-muted-foreground leading-relaxed">{step.body}</p>
          <div className="flex items-center justify-between pt-1">
            <button
              onClick={() => void end()}
              className="text-[11px] text-muted-foreground hover:text-foreground transition-colors"
            >
              Skip tour
            </button>
            <div className="flex items-center gap-1.5">
              {!isFirst && (
                <Button variant="ghost" size="sm" className="h-7 px-2 gap-1" onClick={prev}>
                  <ChevronLeft className="w-3.5 h-3.5" />
                  Back
                </Button>
              )}
              <Button size="sm" className="h-7 px-3 gap-1" onClick={next}>
                {isLast ? 'Finish' : 'Next'}
                {!isLast && <ChevronRight className="w-3.5 h-3.5" />}
              </Button>
            </div>
          </div>
        </motion.div>
      </AnimatePresence>
    </div>,
    document.body
  );
}
