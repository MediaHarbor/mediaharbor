import { useState, useEffect, useRef } from 'react';

interface ITunesResult {
  wrapperType?: string;
  artistName?: string;
  collectionName?: string;
  trackName?: string;
}

type SuggestionType = 'artist' | 'track' | 'album';

interface Suggestion {
  text: string;
  type: SuggestionType;
}

export function useMusicSuggestions(query: string) {
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    const trimmed = query.trim();

    if (trimmed.length < 2) {
      setSuggestions([]);
      return;
    }

    const timer = setTimeout(async () => {
      abortRef.current?.abort();
      abortRef.current = new AbortController();

      try {
        const url =
          `https://itunes.apple.com/search?term=${encodeURIComponent(trimmed)}` +
          `&entity=musicTrack,album,musicArtist&limit=15&media=music`;

        const res = await fetch(url, { signal: abortRef.current.signal });
        const data: { results?: ITunesResult[] } = await res.json();

        const seen = new Set<string>();
        const result: Suggestion[] = [];

        for (const item of data.results ?? []) {
          let entry: Suggestion | null = null;

          if (item.wrapperType === 'artist' && item.artistName) {
            entry = { text: item.artistName, type: 'artist' };
          } else if (item.wrapperType === 'collection' && item.collectionName) {
            entry = { text: item.collectionName, type: 'album' };
          } else if (item.wrapperType === 'track' && item.trackName) {
            entry = { text: item.trackName, type: 'track' };
          }

          if (entry) {
            const key = `${entry.type}:${entry.text.toLowerCase()}`;
            if (!seen.has(key)) {
              seen.add(key);
              result.push(entry);
            }
          }

          if (result.length >= 7) break;
        }

        setSuggestions(result);
      } catch (err: unknown) {
        if (err instanceof Error && err.name !== 'AbortError') setSuggestions([]);
      }
    }, 300);

    return () => clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    if (!query.trim()) setSuggestions([]);
  }, [query]);

  return suggestions;
}
