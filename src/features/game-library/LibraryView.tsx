import { Clock, Plus, Search, Star } from "lucide-react";
import { useMemo, useState } from "react";

import { Button, Input, Segmented, cx } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { formatDuration, timeAgo } from "../../lib/format";
import { NowPlaying } from "../session/NowPlaying";
import { GameCard } from "./GameCard";
import { GameFormModal } from "./GameFormModal";

type Tab = "all" | "favorites" | "recent";

export function LibraryView({ onOpen }: { onOpen: (gameId: string) => void }) {
  const { games, session, recent, toggleFavorite, ready } = useApp();
  const [query, setQuery] = useState("");
  const [platform, setPlatform] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("all");
  const [adding, setAdding] = useState(false);

  const platforms = useMemo(() => [...new Set(games.map((g) => g.platform))].sort(), [games]);
  const recentRank = useMemo(() => new Map(recent.map((r, i) => [r.gameId, i])), [recent]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    let list = games.filter(
      (g) =>
        (!platform || g.platform === platform) &&
        (!q || g.name.toLowerCase().includes(q) || g.platform.toLowerCase().includes(q)),
    );
    if (tab === "favorites") list = list.filter((g) => g.favorite);
    if (tab === "recent") {
      list = list
        .filter((g) => recentRank.has(g.id))
        .sort((a, b) => (recentRank.get(a.id) ?? 0) - (recentRank.get(b.id) ?? 0));
    } else {
      list = [...list].sort((a, b) => Number(b.favorite) - Number(a.favorite) || a.name.localeCompare(b.name));
    }
    return list;
  }, [games, query, platform, tab, recentRank]);

  const recentGames = useMemo(
    () =>
      recent
        .map((r) => ({ stat: r, game: games.find((g) => g.id === r.gameId) }))
        .filter((r) => r.game && r.game.id !== session?.gameId)
        .slice(0, 6),
    [recent, games, session],
  );

  const filtering = query.trim() !== "" || platform !== null || tab !== "all";

  return (
    <div className="space-y-6">
      <NowPlaying onOpen={onOpen} />

      {!filtering && recentGames.length > 0 && (
        <section className="space-y-3">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-muted">
            <Clock className="size-4" /> 최근 플레이한 게임
          </h2>
          <div className="grid grid-cols-3 gap-3 sm:grid-cols-4 lg:grid-cols-6">
            {recentGames.map(({ stat, game }) => (
              <div key={stat.gameId} className="space-y-1">
                <GameCard game={game!} compact playing={false} onOpen={() => onOpen(game!.id)} />
                <p className="px-1 text-[11px] text-muted">
                  {timeAgo(stat.lastPlayedAt, Date.now())} · 총 {formatDuration(stat.totalSecs)}
                </p>
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="space-y-4">
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative min-w-60 flex-1">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted" />
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="게임 이름이나 플랫폼으로 검색"
              className="pl-9"
              aria-label="게임 검색"
            />
          </div>
          <Segmented
            value={tab}
            onChange={setTab}
            options={[
              { value: "all", label: "전체" },
              { value: "favorites", label: "즐겨찾기" },
              { value: "recent", label: "최근" },
            ]}
          />
          <Button variant="primary" onClick={() => setAdding(true)}>
            <Plus className="size-4" /> 게임 추가
          </Button>
        </div>

        {platforms.length > 1 && (
          <div className="flex flex-wrap gap-2">
            {[null, ...platforms].map((p) => (
              <button
                key={p ?? "all"}
                onClick={() => setPlatform(p)}
                className={cx(
                  "rounded-full border px-3 py-1 text-xs font-medium transition-colors",
                  platform === p
                    ? "border-brand bg-brand/20 text-ink"
                    : "border-line text-muted hover:border-muted hover:text-ink",
                )}
              >
                {p ?? "모든 플랫폼"}
              </button>
            ))}
          </div>
        )}

        {visible.length === 0 ? (
          <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-line py-16 text-center">
            {tab === "favorites" ? <Star className="size-8 text-muted" /> : <Search className="size-8 text-muted" />}
            <p className="text-sm text-muted">
              {!ready
                ? "불러오는 중…"
                : games.length === 0
                  ? "라이브러리가 비어 있습니다. 게임을 추가해 보세요."
                  : "조건에 맞는 게임이 없습니다."}
            </p>
          </div>
        ) : (
          <div className="grid gap-4 [grid-template-columns:repeat(auto-fill,minmax(170px,1fr))]">
            {visible.map((game) => (
              <GameCard
                key={game.id}
                game={game}
                playing={session?.gameId === game.id}
                onOpen={() => onOpen(game.id)}
                onToggleFavorite={() => void toggleFavorite(game.id)}
              />
            ))}
          </div>
        )}
      </section>

      {adding && <GameFormModal onClose={() => setAdding(false)} onSaved={(g) => onOpen(g.id)} />}
    </div>
  );
}
