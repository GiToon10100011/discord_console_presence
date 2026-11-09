import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { api, onEvent } from "../lib/api";
import type {
  ConnState,
  DiscordStatus,
  Game,
  PresenceConfig,
  PresenceReport,
  RecentGame,
  Session,
  Settings,
} from "../types";

type ToastKind = "ok" | "error" | "info";
interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

export const DEFAULT_SETTINGS: Settings = {
  applicationId: "",
  autoConnect: true,
  launchAtLogin: false,
  restoreLastSession: true,
  autoClearHours: 12,
  defaultDetails: "Playing",
  defaultState: "",
  defaultImageSource: "discord_asset",
  fallbackAssetKey: "",
  fallbackImageUrl: "",
  overrideActivityName: true,
  imageCacheMaxMb: 200,
};

const IDLE_STATUS: DiscordStatus = {
  state: "idle",
  applicationId: null,
  user: null,
  message: null,
  presenceActive: false,
  lastPresence: null,
  attempts: 0,
  updatedAt: 0,
};

const EMPTY_REPORT: PresenceReport = {
  warnings: [],
  large: { kind: "none", value: null },
  small: { kind: "none", value: null },
};

interface AppContextValue {
  ready: boolean;
  bootError: string | null;
  settings: Settings;
  saveSettings: (next: Settings) => Promise<Settings | undefined>;
  games: Game[];
  addGame: (game: Partial<Game>) => Promise<Game | undefined>;
  updateGame: (game: Game) => Promise<Game | undefined>;
  removeGame: (id: string) => Promise<boolean>;
  toggleFavorite: (id: string) => Promise<void>;
  session: Session | null;
  recent: RecentGame[];
  startSession: (gameId: string, presence?: PresenceConfig) => Promise<boolean>;
  stopSession: () => Promise<void>;
  updatePresence: (presence: PresenceConfig) => Promise<boolean>;
  hidePresence: () => Promise<void>;
  discord: DiscordStatus;
  connect: (applicationId?: string) => Promise<void>;
  disconnect: () => Promise<void>;
  report: PresenceReport;
  /** Bumped whenever cached images may have changed, so views reload. */
  imageVersion: number;
  bumpImages: () => void;
  toast: (text: string, kind?: ToastKind) => void;
  toasts: Toast[];
}

const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const value = useContext(AppContext);
  if (!value) throw new Error("useApp must be used inside <AppProvider>");
  return value;
}

export function connectionLabel(state: ConnState): string {
  switch (state) {
    case "connected":
      return "연결됨";
    case "connecting":
      return "연결 중…";
    case "disconnected":
      return "Discord 대기 중";
    case "error":
      return "연결 오류";
    default:
      return "연결 안 됨";
  }
}

export function AppProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [bootError, setBootError] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [games, setGames] = useState<Game[]>([]);
  const [session, setSession] = useState<Session | null>(null);
  const [recent, setRecent] = useState<RecentGame[]>([]);
  const [discord, setDiscord] = useState<DiscordStatus>(IDLE_STATUS);
  const [report, setReport] = useState<PresenceReport>(EMPTY_REPORT);
  const [imageVersion, setImageVersion] = useState(0);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextToast = useRef(1);

  const toast = useCallback((text: string, kind: ToastKind = "info") => {
    const id = nextToast.current++;
    setToasts((all) => [...all.slice(-3), { id, kind, text }]);
    window.setTimeout(() => setToasts((all) => all.filter((t) => t.id !== id)), 4500);
  }, []);

  /** Runs an action; failures become a toast instead of an unhandled rejection. */
  const attempt = useCallback(
    async <T,>(action: () => Promise<T>, success?: string): Promise<T | undefined> => {
      try {
        const result = await action();
        if (success) toast(success, "ok");
        return result;
      } catch (error) {
        toast(error instanceof Error ? error.message : String(error), "error");
        return undefined;
      }
    },
    [toast],
  );

  const refreshRecent = useCallback(
    () => api.getRecentGames().then(setRecent).catch(() => undefined),
    [],
  );

  useEffect(() => {
    let cancelled = false;
    const unlisten: Array<() => void> = [];

    (async () => {
      try {
        const [loadedSettings, loadedGames, loadedSession, status, loadedReport, recentGames] =
          await Promise.all([
            api.getSettings(),
            api.getGameLibrary(),
            api.getCurrentSession(),
            api.getDiscordStatus(),
            api.getPresenceReport(),
            api.getRecentGames(),
          ]);
        if (cancelled) return;
        setSettings(loadedSettings);
        setGames(loadedGames);
        setSession(loadedSession);
        setDiscord(status);
        setReport(loadedReport);
        setRecent(recentGames);
        setReady(true);

        unlisten.push(await onEvent<DiscordStatus>("discord://status", setDiscord));
        unlisten.push(
          await onEvent<Session | null>("session://changed", (next) => {
            setSession(next);
            void refreshRecent();
          }),
        );
        unlisten.push(await onEvent<PresenceReport>("presence://report", setReport));
      } catch (error) {
        if (!cancelled) setBootError(error instanceof Error ? error.message : String(error));
      }
    })();

    return () => {
      cancelled = true;
      unlisten.forEach((stop) => stop());
    };
  }, [refreshRecent]);

  const value = useMemo<AppContextValue>(
    () => ({
      ready,
      bootError,
      settings,
      saveSettings: async (next) => {
        const saved = await attempt(() => api.updateSettings(next), "설정을 저장했습니다.");
        if (saved) {
          setSettings(saved);
          setImageVersion((v) => v + 1);
        }
        return saved;
      },
      games,
      addGame: async (game) => {
        const added = await attempt(() => api.addGame(game), "게임을 추가했습니다.");
        if (added) setGames((all) => [...all, added]);
        return added;
      },
      updateGame: async (game) => {
        const saved = await attempt(() => api.updateGame(game));
        if (saved) setGames((all) => all.map((g) => (g.id === saved.id ? saved : g)));
        return saved;
      },
      removeGame: async (id) => {
        const done = await attempt(() => api.removeGame(id).then(() => true), "게임을 삭제했습니다.");
        if (done) setGames((all) => all.filter((g) => g.id !== id));
        return !!done;
      },
      toggleFavorite: async (id) => {
        const saved = await attempt(() => api.toggleFavorite(id));
        if (saved) setGames((all) => all.map((g) => (g.id === saved.id ? saved : g)));
      },
      session,
      recent,
      startSession: async (gameId, presence) => {
        const started = await attempt(() => api.startGameSession(gameId, presence));
        if (started) {
          setSession(started);
          void refreshRecent();
        }
        return !!started;
      },
      stopSession: async () => {
        await attempt(() => api.stopGameSession());
        setSession(null);
        void refreshRecent();
      },
      updatePresence: async (presence) => {
        const updated = await attempt(() => api.updatePresence(presence), "Presence를 적용했습니다.");
        if (updated) setSession(updated);
        return !!updated;
      },
      hidePresence: async () => {
        const updated = await attempt(() => api.clearPresence(), "Discord에서 Presence를 숨겼습니다.");
        if (updated) setSession(updated);
      },
      discord,
      connect: async (applicationId) => {
        const status = await attempt(() => api.connectDiscord(applicationId));
        if (status) setDiscord(status);
        if (applicationId !== undefined) {
          const fresh = await attempt(() => api.getSettings());
          if (fresh) setSettings(fresh);
        }
        setImageVersion((v) => v + 1);
      },
      disconnect: async () => {
        const status = await attempt(() => api.disconnectDiscord());
        if (status) setDiscord(status);
      },
      report,
      imageVersion,
      bumpImages: () => setImageVersion((v) => v + 1),
      toast,
      toasts,
    }),
    [
      ready,
      bootError,
      settings,
      games,
      session,
      recent,
      discord,
      report,
      imageVersion,
      toasts,
      attempt,
      toast,
      refreshRecent,
    ],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
