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
}: {
  view?: ImageView;
  name: string;
  loading?: boolean;
  className?: string;
  showStatus?: boolean;
}) {
  const hue = hueOf(name || "?");
  const problem = view && (view.status === "unavailable" || view.status === "error");

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
      {view?.status === "ready" && view.displayPath ? (
        <img
          src={convertFileSrc(view.displayPath)}
          alt={name}
          draggable={false}
          className="size-full object-cover"
        />
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
