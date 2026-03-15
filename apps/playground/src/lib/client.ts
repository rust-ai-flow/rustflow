import { RustFlowClient } from 'rustflow';

/**
 * Singleton SDK client.
 * - HTTP requests: current origin (proxied by Vite in dev)
 * - WebSocket: direct backend address to avoid proxy issues
 */
export const client = new RustFlowClient({
  baseUrl: typeof window !== 'undefined' ? window.location.origin : 'http://localhost:18790',
  wsBaseUrl: 'ws://localhost:18790',
});
