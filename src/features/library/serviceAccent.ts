import type { ServicePlatform } from '@/features/library/api';
import { PLATFORM_COLORS, PLATFORM_LABELS, toClientPlatform } from '@/utils/platform-data';

export function accentFor(source: ServicePlatform): string {
  return PLATFORM_COLORS[toClientPlatform(source)] ?? PLATFORM_COLORS.local;
}

export function labelFor(source: ServicePlatform): string {
  return PLATFORM_LABELS[toClientPlatform(source)] ?? PLATFORM_LABELS.local;
}
