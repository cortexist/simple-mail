/**
 * Tauri event `listen()` that degrades to a no-op outside Tauri.
 *
 * The stores' fire-and-forget `fire*` wrappers all guard with `inTauri()`,
 * but the `subscribe*` wrappers called raw `listen()` — which throws in a
 * plain browser (`window.__TAURI_INTERNALS__` is undefined, so its internal
 * `transformCallback` lookup dies). One of those subscriptions is awaited
 * during startup, so `npm run dev` in a browser hung on "Loading Mail…"
 * forever instead of falling back to mock data.
 *
 * Outside Tauri there is no event source, so a subscription that never fires
 * and an unlisten that does nothing are the correct semantics — mock mode is
 * read-only display.
 */
import { listen as tauriListen, type EventCallback, type UnlistenFn } from '@tauri-apps/api/event';

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export type { UnlistenFn };

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  if (!inTauri) return Promise.resolve(() => {});
  return tauriListen(event, handler);
}
