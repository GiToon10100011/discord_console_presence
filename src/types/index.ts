// Mirrors the serde models in src-tauri (camelCase, times in Unix ms).

export type ImageSource =
  | "discord_asset"
  | "external_url"
  | "uploaded"
  | "local_file"
  | "default";

export interface ImageSpec {
  source: ImageSource;
  assetKey?: string | null;
  url?: string | null;
  localPath?: string | null;
}

export interface Game {
  id: string;
  name: string;
  platform: string;
  details: string;
  state: string;
  imageSource: ImageSource;
  assetKey?: string | null;
  imageUrl?: string | null;
  localImagePath?: string | null;
  largeImageText?: string | null;
  smallImage?: ImageSpec | null;
  smallImageText?: string | null;
  favorite: boolean;
  builtin: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface Settings {
  applicationId: string;
  autoConnect: boolean;
  launchAtLogin: boolean;
  restoreLastSession: boolean;
  autoClearHours: number;
  defaultDetails: string;
  defaultState: string;
  defaultImageSource: ImageSource;
  fallbackAssetKey: string;
  fallbackImageUrl: string;
  overrideActivityName: boolean;
  imageCacheMaxMb: number;
}

export interface PresenceConfig {
  gameName: string;
  details: string;
  state: string;
  largeImage: ImageSpec;
  largeImageText: string;
  smallImage: ImageSpec;
  smallImageText: string;
  startTimestamp: number | null;
}

export interface Session {
  id: string;
  gameId: string;
  gameName: string;
  platform: string;
  startedAt: number;
  lastSeenAt: number;
  presence: PresenceConfig;
  presenceVisible: boolean;
}

export interface SessionRecord {
  id: string;
  gameId: string;
  gameName: string;
  platform: string;
  startedAt: number;
  endedAt: number;
  durationSecs: number;
}

export interface RecentGame {
  gameId: string;
  lastPlayedAt: number;
  totalSecs: number;
  sessions: number;
}

export type ConnState = "idle" | "connecting" | "connected" | "disconnected" | "error";

export interface DiscordUser {
  id: string;
  username: string;
  globalName: string | null;
  avatar: string | null;
}

export interface PresenceOutcome {
  ok: boolean;
  message: string | null;
  at: number;
}

export interface DiscordStatus {
  state: ConnState;
  applicationId: string | null;
  user: DiscordUser | null;
  message: string | null;
  presenceActive: boolean;
  lastPresence: PresenceOutcome | null;
  attempts: number;
  updatedAt: number;
}

export type ImageStatus = "ready" | "none" | "unavailable" | "error";

export interface ImageView {
  status: ImageStatus;
  displayPath: string | null;
  fullPath: string | null;
  width: number | null;
  height: number | null;
  bytes: number | null;
  format: string | null;
  warnings: string[];
  message: string | null;
}

export interface CacheEntry {
  key: string;
  url: string;
  file: string;
  thumbFile: string | null;
  bytes: number;
  width: number | null;
  height: number | null;
  format: string;
  fetchedAt: number;
  lastUsedAt: number;
}

export interface CacheStats {
  entries: number;
  bytes: number;
  maxBytes: number;
  dir: string;
}

export interface CacheOverview {
  stats: CacheStats;
  entries: CacheEntry[];
}

export interface CoverCandidate {
  title: string;
  pageUrl: string;
  fileTitle: string;
  imageUrl: string;
  width: number | null;
  height: number | null;
  mime: string | null;
  warnings: string[];
  source: string;
}

export interface ConnectionTest {
  ok: boolean;
  message: string;
  user: DiscordUser | null;
  assetCount: number | null;
  assetError: string | null;
}

export interface AppAsset {
  id: string;
  name: string;
  kind: number | null;
}

export type AssetCheckStatus = "registered" | "missing" | "unknown";

export interface AssetCheck {
  status: AssetCheckStatus;
  message: string;
}

export type ImageKind = "asset" | "url" | "fallback" | "none";

export interface PresenceReport {
  warnings: string[];
  large: { kind: ImageKind; value: string | null };
  small: { kind: ImageKind; value: string | null };
}

export interface ImportedImage {
  source: ImageSource;
  path: string;
  view: ImageView;
}

export interface AppInfo {
  version: string;
  os: string;
  dataDir: string;
  cacheDir: string;
}
