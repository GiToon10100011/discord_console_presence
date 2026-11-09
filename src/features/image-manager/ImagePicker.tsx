import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Crop, FolderOpen, Link2, RefreshCw, Search, ShieldCheck } from "lucide-react";
import { useState } from "react";

import { Banner, Button, Field, Input, Segmented } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { api } from "../../lib/api";
import { formatBytes } from "../../lib/format";
import type { AssetCheck, ImageSource, ImageSpec } from "../../types";
import { CoverSearch } from "./CoverSearch";
import { CropModal } from "./CropModal";
import { useImageView } from "./useImageView";
import { ViewImage } from "./Thumb";

const PORTAL_URL = "https://discord.com/developers/applications";

/**
 * Chooses where an image comes from. The sources are kept distinct on
 * purpose: only a registered asset key or a public URL can be shown by
 * Discord; local files are previewed here and need one of those two to be
 * visible on Discord.
 */
export function ImagePicker({
  label,
  value,
  onChange,
  gameName,
  optional,
  allowCover,
}: {
  label: string;
  value: ImageSpec;
  onChange: (next: ImageSpec) => void;
  gameName: string;
  /** Small images may be left empty; the large image falls back instead. */
  optional?: boolean;
  allowCover?: boolean;
}) {
  const { imageVersion, bumpImages, toast } = useApp();
  const { view, loading } = useImageView(value, imageVersion);
  const [check, setCheck] = useState<AssetCheck | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [coverOpen, setCoverOpen] = useState(false);
  const [cropOpen, setCropOpen] = useState(false);

  const patch = (changes: Partial<ImageSpec>) => {
    setCheck(null);
    onChange({ ...value, ...changes });
  };

  const run = async (name: string, task: () => Promise<void>) => {
    setBusy(name);
    try {
      await task();
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(null);
    }
  };

  const chooseFile = (mode: Extract<ImageSource, "uploaded" | "local_file">) =>
    run("file", async () => {
      const picked = await open({
        multiple: false,
        filters: [{ name: "이미지", extensions: ["png", "jpg", "jpeg", "webp", "gif", "avif"] }],
      });
      if (typeof picked !== "string") return;
      const imported = await api.importLocalImage(picked, mode);
      bumpImages();
      patch({ source: mode, localPath: imported.path });
    });

  const sources: Array<{ value: ImageSource; label: string }> = [
    { value: "discord_asset", label: "Discord 에셋" },
    { value: "external_url", label: "외부 URL" },
    { value: "uploaded", label: "업로드" },
    { value: "local_file", label: "로컬 파일" },
    { value: "default", label: optional ? "없음" : "기본 이미지" },
  ];

  const isLocal = value.source === "uploaded" || value.source === "local_file";

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted">{label}</span>
        <Segmented
          value={value.source}
          options={sources}
          onChange={(source) => patch({ source })}
        />
      </div>

      <div className="flex gap-4">
        <div className="w-28 shrink-0 space-y-1.5">
          <ViewImage
            view={view}
            name={gameName}
            loading={loading}
            className="aspect-square w-28 rounded-lg"
            showStatus={false}
          />
          {view?.status === "ready" && view.width && view.height && (
            <p className="text-center text-[11px] text-muted">
              {view.width}×{view.height} · {formatBytes(view.bytes ?? 0)}
            </p>
          )}
          {view?.status === "ready" && (
            <Button size="sm" className="w-full" onClick={() => setCropOpen(true)}>
              <Crop className="size-3.5" /> 크롭
            </Button>
          )}
        </div>

        <div className="min-w-0 flex-1 space-y-3">
          {value.source === "discord_asset" && (
            <>
              <Field
                label="Asset Key"
                hint="Developer Portal → Rich Presence → Art Assets에 올린 이름입니다. 키는 소문자로 저장됩니다."
              >
                <div className="flex gap-2">
                  <Input
                    value={value.assetKey ?? ""}
                    onChange={(e) => patch({ assetKey: e.target.value })}
                    placeholder="예: pokemon-za"
                  />
                  <Button
                    busy={busy === "check"}
                    disabled={!value.assetKey?.trim()}
                    onClick={() =>
                      run("check", async () => {
                        const result = await api.checkAssetKey(value.assetKey ?? "");
                        setCheck(result);
                        // The list was just re-read: previews that said "not registered" can retry.
                        if (result.status === "registered") bumpImages();
                      })
                    }
                  >
                    <ShieldCheck className="size-4" /> 확인
                  </Button>
                </div>
              </Field>
              {check && (
                <Banner kind={check.status === "registered" ? "ok" : check.status === "missing" ? "warn" : "info"}>
                  {check.message}
                </Banner>
              )}
              <Button size="sm" variant="ghost" onClick={() => void openUrl(PORTAL_URL)}>
                <Link2 className="size-4" /> Developer Portal 열기
              </Button>
            </>
          )}

          {value.source === "external_url" && (
            <>
              <Field
                label="이미지 URL"
                hint="Discord가 직접 가져오는 공개 https 주소여야 합니다. 앱은 같은 이미지를 내려받아 미리보기와 검증에 사용합니다."
              >
                <div className="flex gap-2">
                  <Input
                    value={value.url ?? ""}
                    onChange={(e) => patch({ url: e.target.value })}
                    placeholder="https://…/cover.png"
                    inputMode="url"
                  />
                  <Button
                    busy={busy === "url"}
                    disabled={!value.url?.trim()}
                    onClick={() =>
                      run("url", async () => {
                        await api.downloadGameImage(value.url ?? "", true);
                        bumpImages();
                        toast("이미지를 확인하고 캐시에 저장했습니다.", "ok");
                      })
                    }
                  >
                    <RefreshCw className="size-4" /> 가져오기
                  </Button>
                </div>
              </Field>
              {allowCover && (
                <Button size="sm" variant="ghost" onClick={() => setCoverOpen(true)}>
                  <Search className="size-4" /> 커버 이미지 찾기
                </Button>
              )}
            </>
          )}

          {isLocal && (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <Button busy={busy === "file"} onClick={() => void chooseFile(value.source as "uploaded" | "local_file")}>
                  <FolderOpen className="size-4" /> 파일 선택
                </Button>
                {view?.fullPath && (
                  <Button size="sm" variant="ghost" onClick={() => void api.revealPath(view.fullPath ?? "")}>
                    파일 위치 열기
                  </Button>
                )}
              </div>
              <p className="text-xs text-muted">
                {value.source === "uploaded"
                  ? "앱이 1024px 이하로 줄인 사본을 보관합니다 (원본을 옮겨도 유지)."
                  : "원본 파일 경로만 기억합니다. 파일을 옮기면 미리보기가 사라집니다."}
              </p>
              <Banner kind="warn">
                <p>
                  Discord는 PC의 파일을 직접 가져올 수 없습니다. 이 이미지를 Developer Portal에 에셋으로 올린 뒤 아래에
                  그 키를 입력하거나, 공개 URL을 사용하세요.
                </p>
              </Banner>
              <Field label="Developer Portal에 올린 Asset Key (선택)">
                <Input
                  value={value.assetKey ?? ""}
                  onChange={(e) => patch({ assetKey: e.target.value })}
                  placeholder="예: my-cover"
                />
              </Field>
            </>
          )}

          {value.source === "default" && (
            <p className="text-sm text-muted">
              {optional
                ? "작은 이미지를 표시하지 않습니다."
                : "설정에서 지정한 기본 이미지를 사용합니다. 지정하지 않으면 Discord 앱 아이콘이 표시됩니다."}
            </p>
          )}

          {view && view.status !== "ready" && view.status !== "none" && view.message && (
            <Banner kind={view.status === "error" ? "error" : "warn"}>{view.message}</Banner>
          )}
          {view?.warnings.map((w) => (
            <p key={w} className="text-xs text-warn">
              {w}
            </p>
          ))}
        </div>
      </div>

      {cropOpen && view && (
        <CropModal spec={value} view={view} gameName={gameName} onClose={() => setCropOpen(false)} />
      )}
      {coverOpen && (
        <CoverSearch
          initialQuery={gameName}
          onClose={() => setCoverOpen(false)}
          onPick={(url) => patch({ source: "external_url", url })}
        />
      )}
    </div>
  );
}
