import { convertFileSrc } from "@tauri-apps/api/core";
import { AlertTriangle, ImageOff } from "lucide-react";

import { cx } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { hueOf, initials } from "../../lib/format";
import type { ImageSpec, ImageView } from "../../types";
import { useImageView } from "./useImageView";

/** Renders an image the backend prepared, or a colored placeholder. */
export function ViewImage({
  view,
  name,
  loading,
  className,
  showStatus = true,
  fit = "frame",
}: {
  view?: ImageView;
  name: string;
  loading?: boolean;
  className?: string;
  showStatus?: boolean;
  /**
   * `frame` shows a small or non-square image at its own size on a blurred
   * copy of itself; `cover` fills the box with a center crop, which is what
   * Discord does.
   */
  fit?: "frame" | "cover";
}) {
  const hue = hueOf(name || "?");
  const problem = view && (view.status === "unavailable" || view.status === "error");
  const src = view?.status === "ready" && view.displayPath ? convertFileSrc(view.displayPath) : null;
  const w = view?.width ?? 0;
  const h = view?.height ?? 0;
  // Stretching a 128px or portrait cover over a square makes it soft and cuts
  // it off, which is what made the library look messy.
  const framed =
    fit === "frame" && !!src && w > 0 && h > 0 && (Math.max(w, h) < 400 || Math.abs(w / h - 1) > 0.08);

  return (
    <div
      className={cx("relative overflow-hidden bg-raised", className)}
      style={
        view?.displayPath
          ? undefined
          : {
              background: `linear-gradient(135deg, hsl(${hue} 45% 32%), hsl(${(hue + 50) % 360} 50% 20%))`,
            }
      }
      title={problem ? (view?.message ?? undefined) : undefined}
    >
      {src && framed ? (
        <>
          <img
            src={src}
            alt=""
            aria-hidden
            draggable={false}
            className="absolute inset-0 size-full scale-125 object-cover opacity-60 blur-2xl"
          />
          <div className="relative flex size-full items-center justify-center p-2">
            <img
              src={src}
              alt={name}
              draggable={false}
              className="rounded-sm object-contain shadow-lg shadow-black/40"
              style={{ maxWidth: `min(100%, ${w}px)`, maxHeight: `min(100%, ${h}px)` }}
            />
          </div>
        </>
      ) : src ? (
        <img src={src} alt={name} draggable={false} className="size-full object-cover" />
      ) : (
        <div className="flex size-full items-center justify-center">
          {loading ? (
            <span className="size-5 animate-spin rounded-full border-2 border-white/40 border-t-transparent" />
          ) : (
            <span className="text-2xl font-bold tracking-wide text-white/70">{initials(name)}</span>
          )}
        </div>
      )}
      {showStatus && problem && (
        <span className="absolute bottom-1.5 left-1.5 flex items-center gap-1 rounded bg-black/65 px-1.5 py-0.5 text-[10px] font-medium text-warn">
          {view?.status === "error" ? <ImageOff className="size-3" /> : <AlertTriangle className="size-3" />}
          {view?.status === "error" ? "이미지 오류" : "이미지 없음"}
        </span>
      )}
    </div>
  );
}

/** An image setting, resolved and shown. */
export function Thumb({
  spec,
  name,
  className,
  showStatus,
}: {
  spec: ImageSpec;
  name: string;
  className?: string;
  showStatus?: boolean;
}) {
  const { imageVersion } = useApp();
  const { view, loading } = useImageView(spec, imageVersion);
  return (
    <ViewImage view={view} name={name} loading={loading} className={className} showStatus={showStatus} />
  );
}
