import {
  Toast,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
} from '@/components/ui/toast';
import { useNotificationStore } from '@/stores/useNotificationStore';

export function Toaster() {
  const notifications = useNotificationStore((state) => state.notifications);
  const removeNotification = useNotificationStore((state) => state.removeNotification);

  return (
    <ToastProvider>
      {notifications.map((notification) => {
        const variant = notification.type === 'error' ? 'error' :
                       notification.type === 'success' ? 'success' :
                       notification.type === 'warning' ? 'warning' :
                       notification.type === 'info' ? 'info' : 'default';

        return (
          <Toast
            key={notification.id}
            variant={variant}
            onOpenChange={(open) => {
              if (!open) {
                removeNotification(notification.id);
              }
            }}
          >
            <div className="grid gap-1">
              {notification.title && <ToastTitle>{notification.title}</ToastTitle>}
              {notification.message && (
                <ToastDescription>{notification.message}</ToastDescription>
              )}
            </div>
            <ToastClose />
          </Toast>
        );
      })}
      <ToastViewport />
    </ToastProvider>
  );
}
