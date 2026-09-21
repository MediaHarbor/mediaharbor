import { QueryClient } from '@tanstack/react-query';

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 5 * 60 * 1000,
    },
  },
});

export const settingsKeys = {
  all: ['settings'] as const,
};

export function invalidateSettingsDerived() {
  void queryClient.invalidateQueries({ queryKey: settingsKeys.all });
  void queryClient.invalidateQueries({ queryKey: ['library'] });
  void queryClient.invalidateQueries({ queryKey: ['service-library'] });
  void queryClient.invalidateQueries({ queryKey: ['search'] });
  void queryClient.invalidateQueries({ queryKey: ['radio'] });
}
