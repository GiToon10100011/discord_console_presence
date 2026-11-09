// Development only (see main.tsx): an in-memory stand-in for the Rust backend,
// used when the UI is opened in a plain browser instead of the Tauri window.
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";

import type {
  DiscordStatus,
  Game,
  ImageSpec,
  ImageView,
  PresenceConfig,
  RecentGame,
  Session,
  Settings,
} from "../types";
import { hueOf, initials } from "./format";

const NOW = Date.now();

const seed = (id: string, name: string, platform: string, details: string, state: string): Game => ({
  id,
  name,
  platform,
  details,
  state,
  imageSource: "discord_asset",
  assetKey: id,
  imageUrl: null,
  localImageUrl: undefined,
  localImagePath: null,
  largeImageText: null,
  smallImage: null,
  smallImageText: null,
  favorite: false,
  builtin: true,
  createdAt: NOW,
  updatedAt: NOW,
} as Game);

function cover(name: string): string {
  const hue = hueOf(name);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="500">
    <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="hsl(${hue} 55% 38%)"/><stop offset="1" stop-color="hsl(${(hue + 60) % 360} 60% 18%)"/>
    </linearGradient></defs>
    <rect width="400" height="500" fill="url(#g)"/>
    <text x="200" y="270" font-family="sans-serif" font-size="96" font-weight="700" fill="rgba(255,255,255,.85)" text-anchor="middle">${initials(name)}</text>
  </svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

// A mix of low-resolution, portrait and high-resolution images, like real covers.
const SIZES = [
  [128, 128],
  [273, 364],
  [1024, 1024],
  [250, 354],
] as const;
const view = (name: string): ImageView => ({
  status: "ready",
  displayPath: cover(name),
  fullPath: null,
  width: SIZES[hueOf(name) % SIZES.length][0],
  height: SIZES[hueOf(name) % SIZES.length][1],
  bytes: 184_320,
  format: "png",
  warnings: [],
  message: null,
});

export function installDevMock() {
  let settings: Settings = {
    applicationId: "123456789012345678",
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
    exportDir: "",
  };
  let games: Game[] = [
    seed("pokemon-za", "Pokémon Legends: Z-A", "Nintendo Switch 2", "Story Progress", "Lumiose City"),
    seed("mario-kart-world", "Mario Kart World", "Nintendo Switch 2", "Grand Prix", "150cc"),
    seed("zelda-totk", "The Legend of Zelda: Tears of the Kingdom", "Nintendo Switch", "Exploring Hyrule", "Hyrule Field"),
    seed("smash-ultimate", "Super Smash Bros. Ultimate", "Nintendo Switch", "Online Battle", "Quickplay"),
    seed("persona-5-royal", "Persona 5 Royal", "PlayStation 5", "Story Progress", "Tokyo"),
    seed("elden-ring", "Elden Ring", "PlayStation 5", "Exploring the Lands Between", "Limgrave"),
    seed("monster-hunter-wilds", "Monster Hunter Wilds", "PlayStation 5", "Hunting", "Windward Plains"),
    seed("cyberpunk-2077", "Cyberpunk 2077", "Xbox Series X|S", "Main Story", "Night City"),
  ];
  games[5].favorite = true;
  let session: Session | null = null;
  const recent: RecentGame[] = [
    { gameId: "elden-ring", lastPlayedAt: NOW - 3 * 3_600_000, totalSecs: 9_200, sessions: 3 },
    { gameId: "zelda-totk", lastPlayedAt: NOW - 26 * 3_600_000, totalSecs: 4_100, sessions: 2 },
  ];
  let discord: DiscordStatus = {
    state: "connected",
    applicationId: settings.applicationId,
    user: { id: "42", username: "tyler", globalName: "Tyler", avatar: null },
    message: null,
    presenceActive: false,
    lastPresence: null,
    attempts: 0,
    updatedAt: NOW,
  };

  const defaults = (game: Game): PresenceConfig => ({
    gameName: game.name,
    details: game.details || settings.defaultDetails,
    state: game.state || settings.defaultState,
    largeImage: {
      source: game.imageSource,
      assetKey: game.assetKey,
      url: game.imageUrl,
      localPath: game.localImagePath,
    },
    largeImageText: game.largeImageText ?? `${game.name} · ${game.platform}`,
    smallImage: game.smallImage ?? { source: "default" },
    smallImageText: game.smallImageText ?? game.platform,
    startTimestamp: null,
  });

  const setDiscord = (changes: Partial<DiscordStatus>) => {
    discord = { ...discord, ...changes, updatedAt: Date.now() };
    void emit("discord://status", discord);
  };
  const setSession = (next: Session | null) => {
    session = next;
    void emit("session://changed", session);
    setDiscord({ presenceActive: !!next?.presenceVisible });
  };
  const find = (id: unknown) => {
    const game = games.find((g) => g.id === id);
    if (!game) throw "게임을 찾을 수 없습니다.";
    return game;
  };

  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, any>;
      switch (cmd) {
        case "get_settings":
          return settings;
        case "update_settings":
          settings = args.settings;
          return settings;
        case "get_game_library":
          return games;
        case "get_current_session":
          return session;
        case "get_discord_status":
          return discord;
        case "get_presence_report":
          return { warnings: [], large: { kind: "asset", value: "x" }, small: { kind: "none", value: null } };
        case "get_recent_games":
          return recent;
        case "get_presence_defaults":
          return defaults(find(args.gameId));
        case "start_game_session": {
          const game = find(args.gameId);
          const now = Date.now();
          setSession({
            id: crypto.randomUUID(),
            gameId: game.id,
            gameName: game.name,
            platform: game.platform,
            startedAt: now,
            lastSeenAt: now,
            presence: args.presence ?? defaults(game),
            presenceVisible: true,
          });
          return session;
        }
        case "update_presence":
          setSession({ ...session!, presence: args.presence, presenceVisible: true });
          return session;
        case "clear_presence":
          setSession({ ...session!, presenceVisible: false });
          return session;
        case "stop_game_session":
          setSession(null);
          return null;
        case "toggle_favorite": {
          const game = find(args.gameId);
          game.favorite = !game.favorite;
          return game;
        }
        case "add_game": {
          const game = { ...seed("new", args.game.name, args.game.platform, "", ""), ...args.game, builtin: false };
          game.id = game.name.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "game";
          games = [...games, game];
          return game;
        }
        case "update_game":
          games = games.map((g) => (g.id === args.game.id ? args.game : g));
          return args.game;
        case "remove_game":
          games = games.filter((g) => g.id !== args.gameId);
          return null;
        case "get_image_view": {
          const spec = args.spec as ImageSpec;
          if (spec.source === "default") return { ...view(""), status: "none", displayPath: null };
          if (spec.source === "discord_asset" && spec.assetKey === "elden-ring")
            return {
              ...view("Elden Ring"),
              warnings: [],
            };
          if (spec.source === "discord_asset" && !["pokemon-za", "zelda-totk", "mario-kart-world"].includes(spec.assetKey ?? ""))
            return {
              ...view(""),
              status: "unavailable",
              displayPath: null,
              message: `Asset Key '${spec.assetKey}'가 Developer Portal에 등록돼 있지 않습니다.`,
            };
          return view(spec.assetKey ?? spec.url ?? "image");
        }
        case "check_asset_key":
          return ["pokemon-za", "zelda-totk", "mario-kart-world", "elden-ring"].includes(args.key)
            ? { status: "registered", message: "Developer Portal에 등록되어 있습니다." }
            : { status: "missing", message: `'${args.key}'는 이 앱에 등록돼 있지 않습니다.` };
        case "test_discord_connection":
          return { ok: true, message: "Discord에 연결되었습니다 (Tyler).", user: discord.user, assetCount: 4, assetError: null };
        case "connect_discord":
          settings = { ...settings, applicationId: args.applicationId ?? settings.applicationId };
          setDiscord({ state: "connected", message: null });
          return discord;
        case "disconnect_discord":
          setDiscord({ state: "idle", message: "연결이 해제되었습니다.", user: null, presenceActive: false });
          return discord;
        case "search_game_cover":
          return [
            {
              title: args.query,
              pageUrl: "https://en.wikipedia.org/wiki/Elden_Ring",
              fileTitle: "File:Elden Ring Box art.jpg",
              imageUrl: "https://upload.wikimedia.org/wikipedia/en/b/b9/Elden_Ring_Box_art.jpg",
              width: 273,
              height: 364,
              mime: "image/jpeg",
              warnings: [
                "해상도가 낮습니다 (273×364). Discord는 1024×1024를 권장합니다.",
                "정사각형이 아니어서 Discord에서 가운데 부분만 잘려 표시될 수 있습니다.",
              ],
              source: "wikipedia",
            },
          ];
        case "download_game_image":
          return view("downloaded");
        case "get_image_cache":
          return {
            stats: { entries: 2, bytes: 412_000, maxBytes: 200 * 1024 * 1024, dir: "~/Library/Caches/com.consolepresence.app/images" },
            entries: [
              { key: "a", url: "https://cdn.discordapp.com/app-assets/1/2.png", file: "a.png", thumbFile: null, bytes: 300_000, width: 1024, height: 1024, format: "png", fetchedAt: NOW, lastUsedAt: NOW },
              { key: "b", url: "local:/Users/me/cover.png:1:2", file: "b.png", thumbFile: null, bytes: 112_000, width: 800, height: 800, format: "png", fetchedAt: NOW, lastUsedAt: NOW - 600_000 },
            ],
          };
        case "clear_image_cache":
          return { entries: 0, bytes: 0, maxBytes: 200 * 1024 * 1024, dir: "" };
        case "export_cropped_image":
          return {
            path: `/mock/exports/${args.name}.png`,
            fileName: `${String(args.name).toLowerCase().replace(/[^a-z0-9_-]+/g, "-")}.png`,
            size: 1024,
            bytes: 482_000,
          };
        case "open_export_dir":
          return "/Users/me/Pictures/Console Presence";
        case "get_app_info":
          return {
            version: "0.1.0",
            os: "browser-mock",
            dataDir: "(mock)",
            cacheDir: "(mock)",
            defaultExportDir: "/Users/me/Pictures/Console Presence",
            exportDir: settings.exportDir || "/Users/me/Pictures/Console Presence",
          };
        default:
          throw `devMock: '${cmd}' is not mocked`;
      }
    },
    { shouldMockEvents: true },
  );

  // Previews are data URLs here, so file paths need no asset-protocol rewrite.
  (window as unknown as { __TAURI_INTERNALS__: { convertFileSrc: (p: string) => string } }).__TAURI_INTERNALS__.convertFileSrc =
    (path) => path;
}
