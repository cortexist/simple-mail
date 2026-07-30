/**
 * renderTrace.ts — opt-in timing for the reading pane's mail render.
 *
 * Exists to answer one open question: why some messages take visibly longer to
 * appear in dark theme than in light. The dark path filters the iframe root,
 * but that filter measured at only +2ms on initial render and +1.2ms per resize
 * in Chromium — far too little to explain it. The engine that actually renders
 * mail here is WebKitGTK, which no browser-side benchmark can stand in for, so
 * the timings have to come from the app itself.
 *
 * Off by default and free when off: a disabled trace is a shared no-op object,
 * so the render path pays one property lookup per event.
 *
 * Enable from the webview devtools (Tauri dev builds have them), then open the
 * same message in each theme and compare:
 *
 *   __mailRenderTrace.enable()      // persists in localStorage
 *   __mailRenderTrace.entries       // completed traces, newest last
 *   __mailRenderTrace.disable()
 *
 * The marks that matter:
 *   write      document.write()+close() returned — synchronous parse only
 *   firstSize  pane first got a height, i.e. the moment content became
 *              visible. Until then the iframe is 0px tall and the mail is
 *              blank however much of it has parsed
 *   load       iframe window 'load' — subresources finished
 *   settled    last height change
 *
 * A dark/light gap concentrated before `firstSize` points at parse or a
 * render-blocking subresource; a gap after it points at paint.
 */

const STORAGE_KEY = 'mailRenderTrace';

export interface RenderTraceContext {
  /** Whether the dark-theme invert was applied to this document. Not the same
   *  as the theme: mail that styles itself for dark backgrounds renders
   *  unfiltered in both. */
  filtered: boolean;
  darkMode: boolean;
  docBytes: number;
  images: number;
}

export interface RenderTrace {
  /** A measurement pass ran. */
  measure(): void;
  /** The iframe height actually changed. */
  resized(height: number): void;
  mark(name: 'write' | 'firstSize' | 'load'): void;
  /** Publish the trace. Safe to call more than once; only the first wins. */
  finish(): void;
}

/** Shared do-nothing trace, used both when tracing is off and as the initial
 *  value before any render has started. */
export const DISABLED_TRACE: RenderTrace = {
  measure() {}, resized() {}, mark() {}, finish() {},
};

interface CompletedTrace extends RenderTraceContext {
  write: number;
  firstSize: number | null;
  load: number | null;
  settled: number | null;
  measures: number;
  resizes: number;
  finalHeight: number | null;
}

let enabled = false;
let installed = false;
const entries: CompletedTrace[] = [];

function readFlag(): boolean {
  try {
    return globalThis.localStorage?.getItem(STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

function writeFlag(on: boolean) {
  try {
    if (on) globalThis.localStorage?.setItem(STORAGE_KEY, '1');
    else globalThis.localStorage?.removeItem(STORAGE_KEY);
  } catch { /* private mode / no storage — the in-memory flag still works */ }
}

const ms = (n: number | null) => (n === null ? '  —  ' : `${n.toFixed(0)}ms`.padStart(6));

function publish(t: CompletedTrace) {
  entries.push(t);
  if (entries.length > 50) entries.shift();
  console.log(
    `[mail-render] ${t.darkMode ? 'dark ' : 'light'}${t.filtered ? ' filtered' : ' native  '}` +
    ` write ${ms(t.write)}  firstSize ${ms(t.firstSize)}  load ${ms(t.load)}  settled ${ms(t.settled)}` +
    `  | ${t.measures} measures, ${t.resizes} resizes, ${t.images} imgs,` +
    ` ${(t.docBytes / 1024).toFixed(0)}KB, ${t.finalHeight ?? '?'}px`,
    t,
  );
}

/** Start timing a render. Returns a no-op when tracing is off. */
export function beginRenderTrace(ctx: RenderTraceContext): RenderTrace {
  if (!enabled) return DISABLED_TRACE;

  const t0 = performance.now();
  const since = () => performance.now() - t0;
  const trace: CompletedTrace = {
    ...ctx,
    write: 0, firstSize: null, load: null, settled: null,
    measures: 0, resizes: 0, finalHeight: null,
  };
  let done = false;

  return {
    measure() { trace.measures++; },
    resized(height: number) {
      trace.resizes++;
      trace.settled = since();
      trace.finalHeight = height;
    },
    mark(name) {
      // First write wins for firstSize; `load` and `write` fire once anyway.
      if (name === 'firstSize' && trace.firstSize !== null) return;
      trace[name] = since();
    },
    finish() {
      if (done) return;
      done = true;
      publish(trace);
    },
  };
}

/** Console handle. Registered unconditionally so tracing can be switched on in
 *  a running build without a rebuild; the object itself costs nothing. */
export function installRenderTraceConsole() {
  // Called from component setup, so it runs again on every remount. The stored
  // flag is read only the first time: re-reading would undo an enable() whose
  // localStorage write was rejected.
  if (installed) return;
  installed = true;
  enabled = readFlag();
  Object.defineProperty(globalThis, '__mailRenderTrace', {
    configurable: true,
    value: {
      get enabled() { return enabled; },
      get entries() { return entries.slice(); },
      enable() { enabled = true; writeFlag(true); console.log('[mail-render] tracing on — reopen a message'); },
      disable() { enabled = false; writeFlag(false); console.log('[mail-render] tracing off'); },
      clear() { entries.length = 0; },
    },
  });
}
