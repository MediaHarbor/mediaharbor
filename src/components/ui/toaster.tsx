import {
  Toast,
  ToastAction,
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
        const variant =
          notification.type === 'error'
            ? 'error'
            : notification.type === 'success'
              ? 'success'
              : notification.type === 'warning'
                ? 'warning'
                : notification.type === 'info'
                  ? 'info'
                  : 'default';

        return (
          <Toast
            key={notification.id}
            variant={variant}
            duration={notification.duration}
            onOpenChange={(open) => {
              if (!open) {
                removeNotification(notification.id);
              }
            }}
          >
            <div className="flex w-full min-w-0 flex-col gap-2">
              <div className="grid min-w-0 gap-1">
                {notification.title && <ToastTitle>{notification.title}</ToastTitle>}
                {notification.message && (
                  <ToastDescription>{notification.message}</ToastDescription>
                )}
              </div>
              {(notification.cta || notification.secondaryCta) && (
                <div className="flex flex-wrap items-center gap-2">
                  {notification.cta && (
                    <ToastAction
                      altText={notification.cta.label}
                      className="border-current/30"
                      onClick={() => {
                        notification.cta?.onClick();
                        removeNotification(notification.id);
                      }}
                    >
                      {notification.cta.label}
                    </ToastAction>
                  )}
                  {notification.secondaryCta && (
                    <ToastAction
                      altText={notification.secondaryCta.label}
                      className="border-transparent bg-transparent px-2 text-current/70 underline-offset-2 hover:bg-current/10 hover:text-current hover:underline"
                      onClick={() => {
                        notification.secondaryCta?.onClick();
                        removeNotification(notification.id);
                      }}
                    >
                      {notification.secondaryCta.label}
                    </ToastAction>
                  )}
                </div>
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
