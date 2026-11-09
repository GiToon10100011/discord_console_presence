import { useApp } from "../../hooks/AppContext";
import { useNow } from "../../hooks/useNow";
import { formatClock } from "../../lib/format";
import type { PresenceConfig } from "../../types";
import { ViewImage } from "../image-manager/Thumb";
import { useImageView } from "../image-manager/useImageView";

/**
 * A mock of the Discord activity card built from the current settings. It is
 * a preview of what is *sent*; how Discord renders it is up to Discord.
 */
export function DiscordPreview({ config, startMs }: { config: PresenceConfig; startMs: number }) {
  const { imageVersion, settings } = useApp();
  const now = useNow(1000);
  const large = useImageView(config.largeImage, imageVersion);
  const small = useImageView(config.smallImage.source === "default" ? null : config.smallImage, imageVersion);
  const hasSmall = small.view?.status === "ready";
  // The same line the app sends: a red-dot badge in front of the state.
  const state = config.live
    ? `🔴 ${settings.streamLabel || "방송 중"}${config.state ? ` · ${config.state}` : ""}`
    : config.state;

  return (
    <div className="rounded-xl bg-[#111214] p-4 shadow-inner">
      <p className="mb-3 text-[11px] font-bold uppercase tracking-wider text-muted">플레이 중</p>
      <div className="flex gap-3.5">
        <div className="relative shrink-0">
          <ViewImage
            view={large.view}
            name={config.gameName}
            loading={large.loading}
            className="size-[84px] rounded-lg"
            showStatus={false}
            fit="cover"
          />
          {hasSmall && (
            <ViewImage
              view={small.view}
              name={config.gameName}
              className="absolute -bottom-1.5 -right-1.5 size-7 rounded-full border-[3px] border-[#111214]"
              showStatus={false}
              fit="cover"
            />
          )}
        </div>
        <div className="min-w-0 flex-1 space-y-0.5 text-[13px] leading-snug">
          <p className="truncate font-bold text-white">{config.gameName || "게임 이름"}</p>
          {config.details && <p className="truncate text-ink">{config.details}</p>}
          {state && <p className="truncate text-ink">{state}</p>}
          <p className="pt-0.5 font-mono tabular-nums text-ok">{formatClock(now - startMs)} 경과</p>
        </div>
      </div>
      {config.live && settings.streamUrl && (
        <div className="mt-3 rounded-md bg-white/10 py-1.5 text-center text-[13px] font-medium text-white">
          {settings.streamButtonLabel || "방송 보러 가기"}
        </div>
      )}
      {config.largeImageText && (
        <p className="mt-3 truncate border-t border-white/5 pt-2 text-[11px] text-muted" title="큰 이미지 툴팁">
          툴팁: {config.largeImageText}
        </p>
      )}
    </div>
  );
}
