import { ExternalLink, Search } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";

import { Badge, Banner, Button, Input, Modal } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { api } from "../../lib/api";
import type { CoverCandidate } from "../../types";

/**
 * Finds cover art on Wikipedia. Search by title is fuzzy, so results are
 * listed for the user to choose from — nothing is applied automatically.
 */
export function CoverSearch({
  initialQuery,
  onPick,
  onClose,
}: {
  initialQuery: string;
  onPick: (url: string) => void;
  onClose: () => void;
}) {
  const { toast, bumpImages } = useApp();
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState<CoverCandidate[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [picking, setPicking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const search = async () => {
    setSearching(true);
    setError(null);
    try {
      setResults(await api.searchGameCover(query));
    } catch (e) {
      setResults(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSearching(false);
    }
  };

  const pick = async (candidate: CoverCandidate) => {
    setPicking(candidate.imageUrl);
    try {
      // Validates the image and keeps a local copy for the preview.
      await api.downloadGameImage(candidate.imageUrl, true);
      bumpImages();
      onPick(candidate.imageUrl);
      onClose();
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setPicking(null);
    }
  };

  return (
    <Modal title="커버 이미지 찾기" onClose={onClose} wide>
      <div className="space-y-4">
        <form
          className="flex gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void search();
          }}
        >
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="게임 제목 (영문 제목이 더 잘 찾아집니다)"
            autoFocus
          />
          <Button type="submit" variant="primary" busy={searching} disabled={query.trim().length < 2}>
            <Search className="size-4" /> 검색
          </Button>
        </form>

        <Banner kind="info">
          <p>
            영어 위키백과 게임 문서의 표지 파일을 찾습니다. 검색은 정확하지 않아 영화·후속작이 섞일 수 있으니
            <b> 목록에서 직접 고르세요</b>. 표지는 작은 이미지인 경우가 많습니다.
          </p>
        </Banner>

        {error && <Banner kind="error">{error}</Banner>}
        {results && results.length === 0 && (
          <p className="py-6 text-center text-sm text-muted">표지를 찾지 못했습니다. 다른 제목으로 검색해 보세요.</p>
        )}

        <ul className="space-y-2">
          {results?.map((c) => (
            <li
              key={c.imageUrl}
              className="flex items-center gap-3 rounded-lg border border-line bg-bg p-3"
            >
              <div className="min-w-0 flex-1 space-y-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="truncate text-sm font-medium">{c.title}</span>
                  {c.width && c.height && (
                    <Badge>
                      {c.width}×{c.height}
                    </Badge>
                  )}
                  {c.warnings.length > 0 && <Badge color="#f0b232">품질 주의</Badge>}
                </div>
                <p className="truncate text-xs text-muted">{c.fileTitle}</p>
                {c.warnings.map((w) => (
                  <p key={w} className="text-xs text-warn">
                    {w}
                  </p>
                ))}
              </div>
              <Button size="sm" variant="ghost" onClick={() => void openUrl(c.pageUrl)} title="위키백과 문서 열기">
                <ExternalLink className="size-4" />
              </Button>
              <Button size="sm" variant="primary" busy={picking === c.imageUrl} onClick={() => void pick(c)}>
                이 이미지 사용
              </Button>
            </li>
          ))}
        </ul>
      </div>
    </Modal>
  );
}
