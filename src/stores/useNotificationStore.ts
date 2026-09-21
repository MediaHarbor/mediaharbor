import { create } from 'zustand';
import { devtools } from 'zustand/middleware';

type NotificationType = 'success' | 'error' | 'info' | 'warning';

interface NotificationCta {
  label: string;
  onClick: () => void;
}

interface Notification {
  id: string;
  type: NotificationType;
  title: string;
  message?: string;
  duration?: number;
  cta?: NotificationCta;
  secondaryCta?: NotificationCta;
  dedupeKey?: string;
}

interface NotificationState {
  notifications: Notification[];
  addNotification: (notification: Omit<Notification, 'id'>) => void;
  removeNotification: (id: string) => void;
  dismissByDedupeKey: (dedupeKey: string) => void;
}

export const useNotificationStore = create<NotificationState>()(
  devtools(
    (set) => ({
      notifications: [],
      addNotification: (notification) => {
        const id = Date.now().toString() + Math.random().toString(36).slice(2, 6);
        const newNotification = { ...notification, id };

        set((state) => ({
          notifications: notification.dedupeKey
            ? [
                ...state.notifications.filter((n) => n.dedupeKey !== notification.dedupeKey),
                newNotification,
              ]
            : [...state.notifications, newNotification],
        }));

        const duration = notification.duration ?? 5000;
        if (duration > 0) {
          setTimeout(() => {
            set((state) => ({
              notifications: state.notifications.filter((n) => n.id !== id),
            }));
          }, duration);
        }
      },
      removeNotification: (id) =>
        set((state) => ({
          notifications: state.notifications.filter((n) => n.id !== id),
        })),
      dismissByDedupeKey: (dedupeKey) =>
        set((state) => ({
          notifications: state.notifications.filter((n) => n.dedupeKey !== dedupeKey),
        })),
    }),
    { name: 'NotificationStore' }
  )
);
