import { Component, type ReactNode, type ErrorInfo } from 'react';
import { useLogStore } from '@/stores/useLogStore';
import { AlertTriangle, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    const detail = `${error.message}\n\nComponent Stack:${errorInfo.componentStack ?? ''}\n\nStack Trace:\n${error.stack ?? 'N/A'}`;
    useLogStore.getState().addLog({
      source: 'app',
      title: `Crash: ${error.message}`,
      fullLog: detail,
      level: 'error',
    });
  }

  handleReload = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center h-full gap-4 p-8 text-center">
          <AlertTriangle className="h-12 w-12 text-destructive" />
          <h2 className="text-lg font-semibold">Something went wrong</h2>
          <p className="text-sm text-muted-foreground max-w-md">
            {this.state.error?.message || 'An unexpected error occurred.'}
          </p>
          <pre className="text-xs bg-muted rounded-md p-4 max-w-lg max-h-48 overflow-auto text-left w-full">
            {this.state.error?.stack || 'No stack trace available.'}
          </pre>
          <Button onClick={this.handleReload} className="gap-2">
            <RefreshCw className="h-4 w-4" /> Try Again
          </Button>
        </div>
      );
    }

    return this.props.children;
  }
}
