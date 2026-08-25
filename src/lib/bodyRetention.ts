/**
 * Which message bodies stay in memory.
 *
 * `Email.body` is a message's full HTML — tens to hundreds of KB each, and only
 * ever rendered for the message currently open. `Email.searchText` is the
 * stripped-down text free-text search runs over, and has to cover every message
 * whether or not it was opened. The two were retained together and neither was
 * released, so the emails array only ever grew: opening a message, scrolling the
 * list, and the background offline downloader all wrote bodies in, and nothing
 * took one out.
 *
 * Bodies now live in a small most-recently-viewed set. Anything evicted falls
 * back to the lazy re-fetch `handleOpen` already performs, which the backend
 * serves out of SQLite — so eviction costs a local round trip, not a network
 * one. `searchText` is deliberately never evicted: dropping it would silently
 * shrink search coverage, which is a worse bug than the memory it costs.
 */

/** Bodies kept resident. Enough to cover reading back through a thread without
 *  a re-fetch, small enough that the total stays bounded by a few MB. */
const MAX_RETAINED_BODIES = 40;

/** Most-recently-viewed last. */
const recentlyViewed: string[] = [];

/**
 * Record that `id`'s body is being retained, and return the ids whose bodies
 * should now be dropped. The id passed in is always the most recent, so the
 * message currently on screen can never be the one evicted.
 */
export function retainBody(id: string): string[] {
  const at = recentlyViewed.indexOf(id);
  if (at !== -1) recentlyViewed.splice(at, 1);
  recentlyViewed.push(id);

  const evicted: string[] = [];
  while (recentlyViewed.length > MAX_RETAINED_BODIES) {
    const dropped = recentlyViewed.shift();
    if (dropped !== undefined) evicted.push(dropped);
  }
  return evicted;
}

/** Drop an id from the retained set — for messages that no longer exist. */
export function releaseBody(id: string): void {
  const at = recentlyViewed.indexOf(id);
  if (at !== -1) recentlyViewed.splice(at, 1);
}

/** Retained-body count. Exposed for tests. */
export function retainedBodyCount(): number {
  return recentlyViewed.length;
}

/** Clear the retained set — for tests and account teardown. */
export function resetBodyRetention(): void {
  recentlyViewed.length = 0;
}
