import { useEffect, useState } from "react";

import { api } from "../../lib/api";
import type { ImageSpec, ImageView } from "../../types";

// A library page asks for many images at once. The backend may hit the
// network for each (Discord CDN, other hosts), so requests are queued.
const MAX_PARALLEL = 3;
let active = 0;
const waiting: Array<() => void> = [];

function limited<T>(task: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const run = () => {
      active += 1;
      task()
        .then(resolve, reject)
        .finally(() => {
          active -= 1;
          waiting.shift()?.();
        });
    };
    if (active < MAX_PARALLEL) run();
    else waiting.push(run);
  });
}

const views = new Map<string, ImageView>();

export function specKey(spec: ImageSpec, version: number): string {
  return JSON.stringify([
    version,
    spec.source,
    spec.assetKey ?? "",
    spec.url ?? "",
    spec.localPath ?? "",
  ]);
}

/**
 * The backend's view of an image setting: where to load it from, plus
 * warnings or the reason it is unavailable. `version` invalidates everything
 * (e.g. after the Application ID or the cache changed).
 */
export function useImageView(spec: ImageSpec | null | undefined, version: number) {
  const key = spec ? specKey(spec, version) : null;
  const [view, setView] = useState<ImageView | undefined>(() => (key ? views.get(key) : undefined));
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!spec || !key) {
      setView(undefined);
      return;
    }
    const cached = views.get(key);
    if (cached) {
      setView(cached);
      return;
    }
    let cancelled = false;
    setLoading(true);
    limited(() => api.getImageView(spec))
      .then((next) => {
        // Only settled answers are remembered. "Unavailable" and errors are
        // not: an asset that was missing a moment ago may have been uploaded
        // since, and a later attempt should be able to succeed.
        if (next.status === "ready" || next.status === "none") views.set(key, next);
        if (!cancelled) setView(next);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setView({
          status: "error",
          displayPath: null,
          fullPath: null,
          width: null,
          height: null,
          bytes: null,
          format: null,
          warnings: [],
          message: error instanceof Error ? error.message : String(error),
        });
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // `key` captures the content of `spec`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return { view, loading };
}
