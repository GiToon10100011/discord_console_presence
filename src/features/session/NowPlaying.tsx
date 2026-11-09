import { EyeOff, Eye, Gamepad2, Pencil, Square } from "lucide-react";

import { Badge, Button } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { useNow } from "../../hooks/useNow";
import { formatClock } from "../../lib/format";
import { platformColor } from "../../lib/platforms";
import { Thumb } from "../image-manager/Thumb";

/** The game being played: elapsed time, Discord state and the stop button. */
export function NowPlaying({ onOpen }: { onOpen: (gameId: string) => void }) {
  const { session, stopSession, hidePresence, updatePresence, discord } = useApp();
  const now = useNow(1000);

  if (!session) {
    return (
      <div className="flex items-center gap-4 rounded-xl border border-dashed border-line bg-panel/50 px-5 py-4">
        <Gamepad2 className="size-8 text-muted" />
        <div>
          <p className="text-sm font-medium">플레이 중인 게임이 없습니다</p>
          <p className="text-xs text-muted">게임을 선택하고 <b>플레이 시작</b>을 누르면 Discord 프로필에 표시됩니다.</p>
        </div>
      </div>
    );
  }

  const start = session.presence.startTimestamp ?? session.startedAt;
  const onDiscord = discord.state === "connected" && discord.presenceActive && session.presenceVisible;

  return (
    <div className="flex flex-wrap items-center gap-5 rounded-xl border border-ok/50 bg-gradient-to-r from-ok/15 via-panel to-panel p-4">
      <Thumb
        spec={session.presence.largeImage}
        name={session.gameName}
        className="size-24 shrink-0 rounded-lg"
        showStatus={false}
      />
      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="flex items-center gap-1.5 text-xs font-semibold text-ok">
            <span className="size-2 animate-[pulse-dot_1.6s_ease-in-out_infinite] rounded-full bg-ok" />
            플레이 중
          </span>
          <Badge color={platformColor(session.platform)}>{session.platform}</Badge>
          {onDiscord ? (
            <Badge color="#23a559">Discord에 표시 중</Badge>
          ) : (
            <Badge color="#f0b232">
              {!session.presenceVisible ? "Discord에서 숨김" : "Discord에 표시되지 않음"}
            </Badge>
          )}
        </div>
        <h2 className="truncate text-xl font-bold">{session.gameName}</h2>
        <p className="truncate text-sm text-muted">
          {[session.presence.details, session.presence.state].filter(Boolean).join(" · ") || "—"}
        </p>
      </div>

      <div className="text-right">
        <p className="font-mono text-3xl font-semibold tabular-nums">{formatClock(now - start)}</p>
        <p className="text-xs text-muted">경과 시간</p>
      </div>

      <div className="flex gap-2">
        <Button onClick={() => onOpen(session.gameId)}>
          <Pencil className="size-4" /> Presence 편집
        </Button>
        {session.presenceVisible ? (
          <Button onClick={() => void hidePresence()} title="세션은 유지하고 Discord에서만 숨깁니다">
            <EyeOff className="size-4" /> 숨기기
          </Button>
        ) : (
          <Button onClick={() => void updatePresence(session.presence)}>
            <Eye className="size-4" /> 다시 표시
          </Button>
        )}
        <Button variant="danger" onClick={() => void stopSession()}>
          <Square className="size-4" /> 종료
        </Button>
      </div>
    </div>
  );
}
