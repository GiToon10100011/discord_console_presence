import { ArrowLeft, Eye, EyeOff, Play, Save, Settings2, Square, Star, Undo2, Zap } from "lucide-react";
import { useEffect, useState } from "react";

import { Badge, Banner, Button, cx } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { useNow } from "../../hooks/useNow";
import { api } from "../../lib/api";
import { formatClock } from "../../lib/format";
import { platformColor } from "../../lib/platforms";
import type { PresenceConfig } from "../../types";
import { GameFormModal } from "../game-library/GameFormModal";
import { DiscordPreview } from "./DiscordPreview";
import { PresenceEditor } from "./PresenceEditor";

export function GameDetail({
  gameId,
  onBack,
  onOpenSettings,
}: {
  gameId: string;
  onBack: () => void;
  onOpenSettings: () => void;
}) {
  const {
    games,
    session,
    discord,
    report,
    startSession,
    stopSession,
    updatePresence,
    hidePresence,
    updateGame,
    toggleFavorite,
    toast,
  } = useApp();
  const game = games.find((g) => g.id === gameId);
  const running = !!game && session?.gameId === game.id;
  const now = useNow(1000);
  const [config, setConfig] = useState<PresenceConfig | null>(null);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);

  // Take the running presence when this game is being played, otherwise the
  // game's defaults. Later session updates do not overwrite unsaved edits.
  useEffect(() => {
    let cancelled = false;
    if (running && session) {
      setConfig(session.presence);
    } else {
      api
        .getPresenceDefaults(gameId)
        .then((defaults) => !cancelled && setConfig(defaults))
        .catch((e: unknown) => toast(e instanceof Error ? e.message : String(e), "error"));
    }
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gameId, running]);

  if (!game) {
    return (
      <div className="space-y-4">
        <Button onClick={onBack}>
          <ArrowLeft className="size-4" /> 라이브러리
        </Button>
        <p className="text-sm text-muted">게임을 찾을 수 없습니다.</p>
      </div>
    );
  }
  if (!config) return <p className="p-8 text-sm text-muted">불러오는 중…</p>;

  const sessionStart = running ? (session?.startedAt ?? null) : null;
  const previewStart = config.startTimestamp ?? sessionStart ?? now;
  const connected = discord.state === "connected";
  const failed = discord.lastPresence && !discord.lastPresence.ok ? discord.lastPresence.message : null;

  const act = async (task: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await task();
    } finally {
      setBusy(false);
    }
  };

  const saveAsDefault = () =>
    act(async () => {
      const saved = await updateGame({
        ...game,
        details: config.details,
        state: config.state,
        imageSource: config.largeImage.source,
        assetKey: config.largeImage.assetKey ?? null,
        imageUrl: config.largeImage.url ?? null,
        localImagePath: config.largeImage.localPath ?? null,
        largeImageText: config.largeImageText || null,
        smallImage: config.smallImage.source === "default" ? null : config.smallImage,
        smallImageText: config.smallImageText || null,
      });
      if (saved) toast("현재 설정을 이 게임의 기본값으로 저장했습니다.", "ok");
    });

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" onClick={onBack}>
          <ArrowLeft className="size-4" /> 라이브러리
        </Button>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-xl font-bold">{game.name}</h1>
          <Badge color={platformColor(game.platform)}>{game.platform}</Badge>
        </div>
        <Button variant="ghost" onClick={() => void toggleFavorite(game.id)} aria-label="즐겨찾기">
          <Star className={cx("size-4", game.favorite && "fill-warn text-warn")} />
        </Button>
        <Button onClick={() => setEditing(true)}>
          <Settings2 className="size-4" /> 게임 정보 수정
        </Button>
      </div>

      {!connected && (
        <Banner kind={discord.state === "error" ? "error" : "warn"}>
          <p>
            <b>Discord에 연결되지 않았습니다.</b> {discord.message ?? "Application ID를 설정하고 Discord 데스크톱 앱을 실행하세요."}
          </p>
          <p className="text-xs text-muted">
            플레이 세션은 그대로 기록되고, 연결되면 Presence가 자동으로 적용됩니다.{" "}
            <button className="text-brand underline" onClick={onOpenSettings}>
              설정 열기
            </button>
          </p>
        </Banner>
      )}
      {failed && <Banner kind="error">{failed}</Banner>}
      {running && report.warnings.length > 0 && (
        <Banner kind="warn">
          {report.warnings.map((w) => (
            <p key={w}>{w}</p>
          ))}
        </Banner>
      )}

      <div className="grid gap-5 lg:grid-cols-[340px_1fr]">
        <aside className="space-y-4 lg:sticky lg:top-4 lg:self-start">
          <DiscordPreview config={config} startMs={previewStart} />

          {running ? (
            <div className="space-y-2">
              <p className="text-center font-mono text-2xl font-semibold tabular-nums text-ok">
                {formatClock(now - (config.startTimestamp ?? session?.startedAt ?? now))}
              </p>
              <Button variant="primary" className="w-full" busy={busy} onClick={() => act(() => updatePresence(config))}>
                <Zap className="size-4" /> Discord에 적용
              </Button>
              <div className="grid grid-cols-2 gap-2">
                {session?.presenceVisible ? (
                  <Button onClick={() => void hidePresence()}>
                    <EyeOff className="size-4" /> 숨기기
                  </Button>
                ) : (
                  <Button onClick={() => act(() => updatePresence(config))}>
                    <Eye className="size-4" /> 다시 표시
                  </Button>
                )}
                <Button variant="danger" onClick={() => act(stopSession)}>
                  <Square className="size-4" /> 플레이 종료
                </Button>
              </div>
            </div>
          ) : (
            <Button
              variant="primary"
              className="w-full py-3 text-base"
              busy={busy}
              onClick={() => act(async () => (await startSession(game.id, config)) && undefined)}
            >
              <Play className="size-5" /> 플레이 시작
            </Button>
          )}

          <div className="grid grid-cols-2 gap-2">
            <Button size="sm" onClick={() => void saveAsDefault()}>
              <Save className="size-4" /> 기본값으로 저장
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                api.getPresenceDefaults(game.id).then(setConfig, (e: unknown) =>
                  toast(e instanceof Error ? e.message : String(e), "error"),
                )
              }
            >
              <Undo2 className="size-4" /> 기본값 불러오기
            </Button>
          </div>
        </aside>

        <PresenceEditor config={config} onChange={setConfig} sessionStart={sessionStart} />
      </div>

      {editing && (
        <GameFormModal
          game={game}
          onClose={() => setEditing(false)}
          onDeleted={onBack}
          onSaved={() => {
            // The defaults changed: reload them unless a session is running.
            if (!running) {
              void api.getPresenceDefaults(game.id).then(setConfig);
            }
          }}
        />
      )}
    </div>
  );
}
