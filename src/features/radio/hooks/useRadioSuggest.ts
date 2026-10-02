import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { RADIO_FRESH, radioKeys } from '@/features/radio/api';
import type { RadioFacet } from '@/features/radio/api';
import { tauriAPI } from '@/tauri-bridge';

const CORPUS_LIMIT = 3000;

/**
 * The whole tag list, cached for a day.
 *
 * It is about 37 KB and half a second to fetch once; keeping it means a
 * suggestion for a prefix costs a filter over an array rather than a request
 * while the user is mid-word.
 */
export function useTagCorpus() {
  return useQuery({
    queryKey: radioKeys.tagCorpus(),
    queryFn: () => tauriAPI.radio.facets('tags', CORPUS_LIMIT),
    staleTime: RADIO_FRESH.facets,
    gcTime: RADIO_FRESH.facets,
  });
}

const NO_FACETS: RadioFacet[] = [];

function rank(facets: RadioFacet[], prefix: string, limit: number): RadioFacet[] {
  const needle = prefix.trim().toLowerCase();
  if (!needle) return NO_FACETS;
  const starts: RadioFacet[] = [];
  const contains: RadioFacet[] = [];
  for (const facet of facets) {
    const name = facet.name.toLowerCase();
    if (name.startsWith(needle)) starts.push(facet);
    else if (name.includes(needle)) contains.push(facet);
    if (starts.length >= limit) break;
  }
  // A prefix hit is what the user is typing towards; a substring hit is a guess.
  return [...starts, ...contains].slice(0, limit);
}

/**
 * Genre suggestions for what is being typed.
 *
 * The cached corpus answers with no network at all. The remote endpoint is the
 * fallback for when that corpus could not be fetched — it is not raced against
 * it, or every keystroke during the corpus's first half-second would fire its
 * own request. It takes the prefix as a *path* segment, because the documented
 * `?filter=` query parameter is ignored and silently returns the global top tags.
 */
export function useTagSuggestions(prefix: string, limit = 8) {
  const corpus = useTagCorpus();
  const trimmed = prefix.trim();
  const haveCorpus = !!corpus.data?.length;

  const remote = useQuery({
    queryKey: radioKeys.suggest(trimmed.toLowerCase()),
    enabled: !haveCorpus && !corpus.isPending && trimmed.length >= 2,
    queryFn: () => tauriAPI.radio.suggest(trimmed, limit),
    staleTime: 60 * 60 * 1000,
  });

  return useMemo(() => {
    if (haveCorpus) return rank(corpus.data ?? NO_FACETS, trimmed, limit);
    return (remote.data ?? NO_FACETS).slice(0, limit);
  }, [haveCorpus, corpus.data, remote.data, trimmed, limit]);
}
