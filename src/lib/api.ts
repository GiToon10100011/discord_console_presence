import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppAsset,
  AppInfo,
  AssetCheck,
  CacheOverview,
  CacheStats,
  ConnectionTest,
  CoverCandidate,
  DiscordStatus,
  Game,
  ImageSource,
  ImageSpec,
  ImageView,
  ImportedImage,
  PresenceConfig,
  PresenceReport,
  RecentGame,
  Session,
  SessionRecord,
  Settings,
} from "../types";

// Thin, typed wrappers over the Tauri commands. Rust errors arrive as plain
// strings and are rethrown as Error so callers can show `error.message`.
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}

export const api = {
  // Discord
  getDiscordStatus: () => call<DiscordStatus>("get_discord_status"),
  connectDiscord: (applicationId?: string) =>
    call<DiscordStatus>("connect_discord", { applicationId }),
  disconnectDiscord: () => call<DiscordStatus>("disconnect_discord"),
  testDiscordConnection: (applicationId?: string) =>
    call<ConnectionTest>("test_discord_connection", { applicationId }),
  listDiscordAssets: (refresh = false) => call<AppAsset[]>("list_discord_assets", { refresh }),
  checkAssetKey: (key: string) => call<AssetCheck>("check_asset_key", { key }),

  // Session / presence
  getCurrentSession: () => call<Session | null>("get_current_session"),
  getPresenceDefaults: (gameId: string) =>
    call<PresenceConfig>("get_presence_defaults", { gameId }),
  getPresenceReport: () => call<PresenceReport>("get_presence_report"),
  startGameSession: (gameId: string, presence?: PresenceConfig) =>
    call<Session>("start_game_session", { gameId, presence }),
  stopGameSession: () => call<SessionRecord | null>("stop_game_session"),
  updatePresence: (presence: PresenceConfig) => call<Session>("update_presence", { presence }),
  clearPresence: () => call<Session>("clear_presence"),
  getRecentGames: (limit = 12) => call<RecentGame[]>("get_recent_games", { limit }),
  getSessionHistory: (limit = 50) => call<SessionRecord[]>("get_session_history", { limit }),

  // Library
  getGameLibrary: () => call<Game[]>("get_game_library"),
  addGame: (game: Partial<Game>) => call<Game>("add_game", { game }),
  updateGame: (game: Game) => call<Game>("update_game", { game }),
  toggleFavorite: (gameId: string) => call<Game>("toggle_favorite", { gameId }),
  removeGame: (gameId: string) => call<void>("remove_game", { gameId }),

  // Images
  getImageView: (spec: ImageSpec) => call<ImageView>("get_image_view", { spec }),
  downloadGameImage: (url: string, force = false) =>
    call<ImageView>("download_game_image", { url, force }),
  getImageCache: () => call<CacheOverview>("get_image_cache"),
  clearImageCache: () => call<CacheStats>("clear_image_cache"),
  importLocalImage: (path: string, mode: Extract<ImageSource, "uploaded" | "local_file">) =>
    call<ImportedImage>("import_local_image", { path, mode }),
  searchGameCover: (query: string) => call<CoverCandidate[]>("search_game_cover", { query }),
  revealPath: (path: string) => call<void>("reveal_path", { path }),

  // Settings
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) => call<Settings>("update_settings", { settings }),
  getAppInfo: () => call<AppInfo>("get_app_info"),
};

export type EventName = "discord://status" | "session://changed" | "presence://report";

export function onEvent<T>(name: EventName, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (event) => handler(event.payload));
}
