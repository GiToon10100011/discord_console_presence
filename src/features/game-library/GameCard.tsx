import { Star } from "lucide-react";

import { Badge, cx } from "../../components/ui";
import { platformColor } from "../../lib/platforms";
import type { Game } from "../../types";
import { Thumb } from "../image-manager/Thumb";

export function GameCard({
  game,
  playing,
  compact,
  onOpen,
  onToggleFavorite,
}: {
  game: Game;
  playing: boolean;
  compact?: boolean;
  onOpen: () => void;
  onToggleFavorite?: () => void;
}) {
  return (
    <div
      className={cx(
        "group relative overflow-hidden rounded-xl border bg-panel transition-all",
        "hover:-translate-y-0.5 hover:border-brand/60 hover:shadow-lg hover:shadow-black/30",
        playing ? "border-ok/70 ring-1 ring-ok/40" : "border-line",
      )}
    >
      <button
        onClick={onOpen}
        className="block w-full text-left focus-visible:outline-2 focus-visible:outline-brand"
        aria-label={`${game.name} 열기`}
      >
        <Thumb
          spec={game.imageSource === "default" ? { source: "default" } : {
            source: game.imageSource,
            assetKey: game.assetKey,
            url: game.imageUrl,
            localPath: game.localImagePath,
          }}
          name={game.name}
          className={cx("w-full", compact ? "aspect-square" : "aspect-[4/5]")}
        />
        <div className="space-y-1.5 p-3">
          <h3 className={cx("line-clamp-2 font-semibold leading-snug", compact ? "text-xs" : "text-sm")}>
            {game.name}
          </h3>
          <Badge color={platformColor(game.platform)}>{game.platform}</Badge>
        </div>
      </button>

      {playing && (
        <span className="absolute left-2 top-2 flex items-center gap-1.5 rounded-full bg-ok px-2 py-0.5 text-[11px] font-semibold text-white shadow">
          <span className="size-1.5 animate-[pulse-dot_1.6s_ease-in-out_infinite] rounded-full bg-white" />
          플레이 중
        </span>
      )}

      {onToggleFavorite && (
        <button
          onClick={onToggleFavorite}
          aria-label={game.favorite ? "즐겨찾기 해제" : "즐겨찾기"}
          className={cx(
            "absolute right-2 top-2 rounded-full bg-black/55 p-1.5 transition-opacity",
            game.favorite ? "opacity-100" : "opacity-0 group-hover:opacity-100",
          )}
        >
          <Star className={cx("size-4", game.favorite ? "fill-warn text-warn" : "text-white")} />
        </button>
      )}
    </div>
  );
}
