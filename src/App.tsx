import { Gamepad2, Settings as SettingsIcon } from "lucide-react";
import { useState } from "react";

import { Banner, Button, cx } from "./components/ui";
import { LibraryView } from "./features/game-library/LibraryView";
import { GameDetail } from "./features/presence/GameDetail";
import { SettingsView } from "./features/settings/SettingsView";
import { AppProvider, connectionLabel, useApp } from "./hooks/AppContext";

type View = { kind: "library" } | { kind: "game"; id: string } | { kind: "settings" };

const DOT: Record<string, string> = {
  connected: "bg-ok",
  connecting: "bg-warn animate-[pulse-dot_1.2s_ease-in-out_infinite]",
  disconnected: "bg-warn",
  error: "bg-bad",
  idle: "bg-muted",
};

function StatusPill({ onClick }: { onClick: () => void }) {
  const { discord } = useApp();
  const name = discord.user?.globalName ?? discord.user?.username;
  return (
    <button
      onClick={onClick}
      title={discord.message ?? "Discord 연결 상태 (클릭하면 설정으로 이동)"}
      className="flex items-center gap-2 rounded-full border border-line bg-panel px-3.5 py-1.5 text-xs font-medium hover:bg-raised"
    >
      <span className={cx("size-2 rounded-full", DOT[discord.state])} />
      <span>Discord · {connectionLabel(discord.state)}</span>
      {name && discord.state === "connected" && <span className="text-muted">({name})</span>}
    </button>
  );
}

function Toasts() {
  const { toasts } = useApp();
  return (
    <div className="pointer-events-none fixed bottom-16 right-5 z-[60] flex w-80 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          role="status"
          className={cx(
            "rounded-lg border px-4 py-2.5 text-sm shadow-xl",
            t.kind === "error" && "border-bad/50 bg-[#3b1f21] text-ink",
            t.kind === "ok" && "border-ok/50 bg-[#1c3328] text-ink",
            t.kind === "info" && "border-line bg-raised text-ink",
          )}
        >
          {t.text}
        </div>
      ))}
    </div>
  );
}

function Shell() {
  const { ready, bootError, settings } = useApp();
  const [view, setView] = useState<View>({ kind: "library" });

  if (bootError) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="max-w-lg space-y-3 text-center">
          <h1 className="text-lg font-bold">앱 백엔드에 연결하지 못했습니다</h1>
          <p className="text-sm text-muted">{bootError}</p>
          <p className="text-xs text-muted">
            이 화면은 Tauri 앱 안에서 열려야 합니다. 개발 중이라면 <code>npm run tauri dev</code>로 실행하세요.
          </p>
        </div>
      </div>
    );
  }
  if (!ready) {
    return (
      <div className="flex h-full items-center justify-center">
        <span className="size-8 animate-spin rounded-full border-2 border-brand border-t-transparent" />
      </div>
    );
  }

  const openSettings = () => setView({ kind: "settings" });

  return (
    <div className="flex h-full flex-col">
      <header className="flex shrink-0 items-center justify-between gap-4 border-b border-line bg-panel px-6 py-3">
        <button onClick={() => setView({ kind: "library" })} className="flex items-center gap-2.5">
          <span className="flex size-8 items-center justify-center rounded-lg bg-brand">
            <Gamepad2 className="size-5 text-white" />
          </span>
          <span className="text-base font-bold tracking-tight">Console Presence</span>
        </button>
        <div className="flex items-center gap-2.5">
          <StatusPill onClick={openSettings} />
          <Button
            variant={view.kind === "settings" ? "primary" : "ghost"}
            onClick={openSettings}
            aria-label="설정"
          >
            <SettingsIcon className="size-4" /> 설정
          </Button>
        </div>
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-6xl space-y-5 px-6 py-6">
          {view.kind === "library" && !settings.applicationId && (
            <Banner kind="info">
              <p>
                <b>Discord Application ID를 설정하면 시작할 수 있습니다.</b> 설정 화면에서 ID를 입력하고 연결하세요.
              </p>
              <Button size="sm" variant="primary" onClick={openSettings}>
                설정 열기
              </Button>
            </Banner>
          )}
          {view.kind === "library" && <LibraryView onOpen={(id) => setView({ kind: "game", id })} />}
          {view.kind === "game" && (
            <GameDetail
              gameId={view.id}
              onBack={() => setView({ kind: "library" })}
              onOpenSettings={openSettings}
            />
          )}
          {view.kind === "settings" && <SettingsView />}
        </div>
      </main>
      <Toasts />
    </div>
  );
}

export default function App() {
  return (
    <AppProvider>
      <Shell />
    </AppProvider>
  );
}
