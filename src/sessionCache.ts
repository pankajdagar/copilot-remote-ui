export const MAX_CACHED_VIEWS = 4;

export function touchSessionCache(ids: string[], id: string, limit = MAX_CACHED_VIEWS): string[] {
  if (ids.at(-1) === id) return ids;
  return [...ids.filter((current) => current !== id), id].slice(-limit);
}
