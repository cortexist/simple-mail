/**
 * emailBody.ts — unwrap a received mail's HTML so it can be re-wrapped inside
 * the reading pane's own controlled document.
 *
 * The rule is subtractive: remove document scaffolding, keep everything else in
 * document order. Earlier versions extracted the span *between* `<body>` and
 * `</body>` instead, which silently deleted mail whenever a message wasn't the
 * single well-formed document that shape assumes — and real mail routinely
 * isn't:
 *
 *   - Drag-and-drop builders (Medium, 3DEXPERIENCE) drop entire
 *     `<!DOCTYPE html>…</html>` documents into an "HTML block" cell, so the
 *     first `</body>` arrives a fifth of the way in and a lazy match threw away
 *     the rest — ~90% of a Medium newsletter, leaving a pane with no text.
 *   - Forwarding clients wrap the original message in a `<div>`, putting the
 *     "---------- Forwarded message ----------  From:/Date:/Subject:/To:"
 *     attribution *before* the only `<body>` tag, where extraction never
 *     reached it.
 *
 * Both failures render a well-formed, correctly-sized document that is simply
 * missing the mail, so nothing downstream can notice. `scripts/check-body-unwrap.mjs`
 * asserts the invariant directly — run it after changing this file.
 *
 * Lives outside ReadingPane.svelte so that script can exercise the real
 * transform. The regexes were previously built with `new RegExp('<' + '/body>')`
 * concatenation to keep literal closing tags away from the Svelte parser; in a
 * plain .ts module that constraint is gone and they can be ordinary literals.
 */

/** Does this body look like a whole document rather than a fragment?
 *  Only documents go through {@link unwrapHtmlDocument}. */
export function isHtmlDocument(body: string): boolean {
  return /<!doctype|<html[\s>]/i.test(body);
}

/** Disarm the mail's own dark-mode rules.
 *
 *  `prefers-color-scheme` inside the iframe answers to the OS, not to the app's
 *  theme toggle, so leaving these live makes rendering nondeterministic — and
 *  mail dark support is usually partial, which turns that into unreadable
 *  hybrids rather than a clean second design. Real example, three ChatGPT
 *  newsletters: they flip body colours under `prefers-color-scheme: dark` but
 *  leave inner table backgrounds light. With the OS in light mode the pane
 *  showed the mail's dark text with no canvas behind it; with the OS in dark
 *  mode the same mail measured white text on a white table — 1:1 contrast
 *  either way, from one document.
 *
 *  Rewriting the query to one that can never match pins every message to its
 *  light design, which is the invariant the rest of the pipeline assumes:
 *  `ensureReadableTextColors` culls colours unreadable on white, and dark theme
 *  is an invert of the white rendering. One path, same result on every machine.
 *
 *  `[data-ogsc]`/`[data-ogsb]` (Outlook's dark-mode hooks) are left alone —
 *  those attributes only exist if Outlook injects them, so they never match
 *  here. */
function disarmDarkModeQueries(html: string): string {
  return html.replace(/prefers-color-scheme\s*:\s*dark/gi, 'prefers-color-scheme:__never');
}

const RE_STYLE_G = /<style[\s\S]*?<\/style>/gi;

// `(?:\s[^>]*)?` rather than `[^>]*` throughout, so `<head…>` doesn't also
// match `<header>` — mail templates use both.
const RE_HEAD_BLOCK_G = /<head(?:\s[^>]*)?>[\s\S]*?<\/head>/gi;

// Document scaffolding, including the stragglers of any nested document. Tags
// only: the contents stay, so a nested `<body>`'s markup survives intact.
const RE_SCAFFOLD_G = /<!doctype[^>]*>|<\/?(?:html|head|body)(?:\s[^>]*)?>/gi;

/** Remove `<style>` blocks. Used by the print path, which renders into the
 *  app's own DOM where the mail's stylesheets would leak out of the message. */
export function stripStyleBlocks(html: string): string {
  return html.replace(RE_STYLE_G, '');
}

/** Strip the HTML/HEAD/BODY scaffolding from an email so we can re-wrap it in
 *  our own controlled document.
 *
 *  Each `<head>` collapses to just its `<style>` blocks, **left where it sat**.
 *  Hoisting them all to the front instead would reorder the cascade: a nested
 *  document's styles would start winning ties against outer rules they were
 *  never meant to outrank, since equal-specificity conflicts are settled by
 *  source order and mail templates are full of them. In place, the outermost
 *  head still lands ahead of all content, exactly as before.
 *
 *  What is dropped is the rest of the head — metadata, and notably
 *  `<link rel="stylesheet">`: a remote font stylesheet is render-blocking
 *  inside the iframe and the pane can't paint until it resolves. */
export function unwrapHtmlDocument(html: string): string {
  // Everything outside a head block survives, in order — that is what makes
  // this transform incapable of losing the message.
  return html.trim()
    .replace(RE_HEAD_BLOCK_G, (head) => (head.match(RE_STYLE_G) || []).join('\n'))
    .replace(RE_SCAFFOLD_G, '');
}

/** The canvas colour the mail expects behind it.
 *
 *  Mail declares this on the `<body>` tag — `bgcolor="#FFFFFF"` or an inline
 *  `background-color` — and {@link unwrapHtmlDocument} strips that tag along
 *  with the rest of the document scaffolding. Losing it leaves the iframe
 *  transparent, so whatever the app paints behind the pane shows through: in
 *  dark theme that put the mail's own dark text on the pane's dark grey, with
 *  no contrast at all.
 *
 *  Defaults to white, which is the invariant the rest of this pipeline is built
 *  on — `ensureReadableTextColors` culls colours unreadable on white, and dark
 *  theme is an invert of the white rendering. Mail that paints its own canvas
 *  from a `<style>` block still overrides this: those rules target our `<body>`
 *  and land later in the cascade. */
function canvasColor(html: string): string {
  const bodyTag = html.match(/<body(?:\s[^>]*)?>/i)?.[0];
  if (!bodyTag) return '#ffffff';
  const attr = bodyTag.match(/\bbgcolor\s*=\s*["']?(#?[\w(),.\s%-]+)["']?/i)?.[1];
  const inline = bodyTag.match(/background-color\s*:\s*([^;"']+)/i)?.[1];
  return (inline || attr || '#ffffff').trim();
}

/** Mark images so they never hold up the first paint.
 *
 *  `decoding="async"` keeps image decode off the path to painting text, and
 *  `loading="lazy"` defers anything below the fold — which, before the pane has
 *  been sized, is everything past the first screenful. The text lays out and
 *  the pane sizes itself against it; images then fill in as they arrive.
 *
 *  Existing attributes win: a mail that already asks for eager loading keeps
 *  it. */
function deferImageLoading(html: string): string {
  return html
    .replace(/<img\b(?![^>]*\bloading\s*=)/gi, '<img loading="lazy"')
    .replace(/<img\b(?![^>]*\bdecoding\s*=)/gi, '<img decoding="async"');
}

/** Options for {@link buildMailDocument}. */
export interface MailDocumentOptions {
  darkMode?: boolean;
  /** Junk mail renders under a CSP that blocks every external resource. */
  isJunk?: boolean;
  /** Inline-color cleanup, injected so this module stays free of app imports
   *  and the render harness can measure with it in place. */
  ensureReadableTextColors?: (html: string) => string;
}

/** Assemble the self-contained document the reading pane writes into its
 *  iframe. Kept pure and out of the component so a harness can render and
 *  measure exactly what the app renders, rather than a reconstruction of it. */
export function buildMailDocument(body: string, opts: MailDocumentOptions = {}): string {
  const { darkMode = false, isJunk = false, ensureReadableTextColors = (h: string) => h } = opts;
  const unwrapped = disarmDarkModeQueries(isHtmlDocument(body) ? unwrapHtmlDocument(body) : body);
  // Colors are authored against white here (dark theme is an invert of the
  // white rendering), so drop inline text colors unreadable on white —
  // e.g. dark-theme-editor pastes. Native-dark mail manages its own colors.
  const content = deferImageLoading(ensureReadableTextColors(unwrapped));
  // Dark theme renders the white document through an invert, then inverts
  // pictorial content back. `filter` on the root puts the whole document into
  // its own render surface, so every height change repaints through the filter.
  // Measured on a 4631px, 79-image digest in Chromium: +2ms on initial render
  // and +1.2ms per resize versus light, essentially all of it from the
  // per-image counter-filter rather than the root one. Small, but it is why the
  // resize path is worth keeping quiet (see makeResizeScheduler).
  const darkCss = darkMode ? `
      html{filter:invert(0.9) hue-rotate(170deg);}
      img,video,picture,svg,canvas,[style*="background-image"]{filter:invert(0.9) hue-rotate(170deg);}
    ` : '';
  // CSP is what stops mail from running scripts, and it is emitted before any
  // mail markup is parsed — that ordering is load-bearing, so keep this first
  // in <head>.
  //
  // It replaces the iframe sandbox for that job. The pane needs `allow-scripts`
  // because WebKitGTK refuses to invoke *any* listener on a script-less
  // sandboxed document — including the ones the app registers from the parent
  // realm — which silently killed link interception, key forwarding, focus
  // notification and hover preview, and logged a security error per event.
  // Verified in WebKitGTK 2.52: under this policy a mail <script> and an inline
  // on* handler are both refused, inline <style> still applies, and the app's
  // own listeners run.
  //
  // Junk additionally loses remote images; everything else keeps them.
  const imgSrc = isJunk ? 'data:' : 'data: https: http:';
  const cspMeta = '<meta http-equiv="Content-Security-Policy" content="' +
    "default-src 'none'; " +
    "script-src 'none'; object-src 'none'; frame-src 'none'; child-src 'none'; " +
    "form-action 'none'; base-uri 'none'; " +
    "style-src 'unsafe-inline'; font-src data:; " +
    `img-src ${imgSrc}; media-src ${imgSrc};` +
    '">';
  const css = 'html,body{margin:0;padding:0;height:auto!important;overflow:visible!important;min-height:0!important;}' +
    // Pinned light: the mail's own dark rules are disarmed, so UA-supplied
    // colours (form controls, default text) must match the light design too.
    `html{color-scheme:light;}html,body{background-color:${canvasColor(body)};}` +
    'body{font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;font-size:14px;line-height:1.65;color:#242424;overflow-wrap:break-word;}' +
    'img{max-width:100%;height:auto;}a{color:#0078d4;}' + darkCss;
  // The geometry rules repeat AFTER the content. The email's own stylesheets
  // land between the two, so on equal-specificity ties (`body{height:100%
  // !important}` is boilerplate in real mail) source order decides — and
  // resizeIframe can only trust document geometry if these rules win.
  // Only the geometry subset: theming (fonts, link color) stays first so
  // emails can still override it.
  //
  // .__mailSqueeze: fixed-width mail (`<table width="640">`, images with
  // inline `width: 640px`) refuses to shrink below its min-content floor,
  // and the iframe is scrolling="no" — so in a pane narrower than the mail,
  // the right edge was silently clipped mid-word. When resizeIframe detects
  // horizontal overflow it stamps this class on <html>, and these rules
  // relax the fixed widths so the layout adapts the way Gmail/Outlook web
  // render the same mail. Conditional on purpose: `img{width:auto}` is the
  // piece that actually breaks the floor (nothing else beats an inline
  // pixel width during table min-content sizing), but globally it would
  // blow retina images (1280px assets declared width=640) up to their
  // natural size — mail that fits must keep author-exact rendering.
  const geometryCss = 'html,body{height:auto!important;min-height:0!important;overflow:visible!important;}' +
    'html.__mailSqueeze table{width:auto!important;max-width:100%!important;}' +
    'html.__mailSqueeze img{width:auto!important;max-width:100vw!important;height:auto!important;}';
  return '<!DOCTYPE html><html><head><meta charset="utf-8">' + cspMeta +
    '<style>' + css + '</style></head>' +
    '<body>' + content +
    '<style>' + geometryCss + '</style>' +
    '</body></html>';
}
