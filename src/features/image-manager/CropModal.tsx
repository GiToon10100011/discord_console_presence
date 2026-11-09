import { convertFileSrc } from "@tauri-apps/api/core";
import { FolderOpen, LocateFixed, Maximize2 } from "lucide-react";
import { useRef, useState } from "react";

import { Banner, Button, Field, Input, Modal } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { api } from "../../lib/api";
import type { ExportedImage, ImageSpec, ImageView } from "../../types";

const STAGE_W = 540;
const STAGE_H = 380;
const PORTAL = "https://discord.com/developers/applications";

/**
 * Picks the square Discord should show. Discord crops a non-square image to
 * its center and has no setting for it, so the app cuts the chosen square out
 * of the original and exports it as a 1024px PNG to upload as the asset.
 */
export function CropModal({
  spec,
  view,
  gameName,
  onClose,
}: {
  spec: ImageSpec;
  view: ImageView;
  gameName: string;
  onClose: () => void;
}) {
  const { toast } = useApp();
  const width = view.width ?? 0;
  const height = view.height ?? 0;
  const path = view.fullPath ?? view.displayPath;
  const src = path ? convertFileSrc(path) : null;
  const usable = !!src && width > 0 && height > 0;

  const short = Math.min(width, height);
  const minSize = Math.min(short, Math.max(16, Math.round(short * 0.1)));
  const [size, setSize] = useState(short);
  // Starts where Discord's own automatic crop would be: the centered square.
  const [pos, setPos] = useState({
    x: Math.floor((width - short) / 2),
    y: Math.floor((height - short) / 2),
  });
  const [name, setName] = useState(spec.assetKey?.trim() || gameName);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ExportedImage | null>(null);
  const drag = useRef<{ px: number; py: number; x: number; y: number } | null>(null);

  const scale = usable ? Math.min(STAGE_W / width, STAGE_H / height) : 1;
  const clamp = (x: number, y: number, s: number) => ({
    x: Math.min(Math.max(0, Math.round(x)), width - s),
    y: Math.min(Math.max(0, Math.round(y)), height - s),
  });

  const resize = (next: number) => {
    // Grow and shrink around the middle of the selection.
    const centerX = pos.x + size / 2;
    const centerY = pos.y + size / 2;
    setSize(next);
    setPos(clamp(centerX - next / 2, centerY - next / 2, next));
  };
  const center = () =>
    setPos(clamp((width - size) / 2, (height - size) / 2, size));

  const exportIt = async () => {
    setBusy(true);
    try {
      setResult(await api.exportCroppedImage(spec, { x: pos.x, y: pos.y, size }, name));
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const preview = (px: number, round?: boolean) => (
    <div
      className={round ? "rounded-full" : "rounded-lg"}
      style={{
        width: px,
        height: px,
        // Quoted: an unquoted url() breaks on parentheses and spaces in the address.
        backgroundImage: `url(${JSON.stringify(src)})`,
        backgroundRepeat: "no-repeat",
        backgroundSize: `${(width * px) / size}px ${(height * px) / size}px`,
        backgroundPosition: `${-(pos.x * px) / size}px ${-(pos.y * px) / size}px`,
      }}
    />
  );

  return (
    <Modal title="Discord에 보일 영역 크롭" onClose={onClose} wide>
      {!usable ? (
        <Banner kind="warn">이 이미지는 크기를 알 수 없어(AVIF 등) 자를 수 없습니다.</Banner>
      ) : (
        <div className="space-y-4">
          <p className="text-sm text-muted">
            Discord는 큰 이미지를 정사각형으로 보여 주고, 직사각형이면 가운데를 기준으로 알아서 자릅니다. 아래에서
            <b> 원하는 정사각형 영역</b>을 정하면 1024×1024 PNG로 내보냅니다. 처음 위치는 Discord가 자동으로 자르는 영역과
            같습니다.
          </p>

          <div className="flex flex-wrap gap-5">
            <div className="space-y-3">
              <div
                className="relative select-none overflow-hidden rounded-lg bg-black"
                style={{ width: width * scale, height: height * scale }}
              >
                <img src={src} alt="" draggable={false} className="absolute inset-0 size-full" />
                <div
                  className="absolute cursor-move border-2 border-white shadow-[0_0_0_9999px_rgba(0,0,0,0.6)]"
                  style={{
                    left: pos.x * scale,
                    top: pos.y * scale,
                    width: size * scale,
                    height: size * scale,
                    touchAction: "none",
                  }}
                  onPointerDown={(e) => {
                    e.currentTarget.setPointerCapture(e.pointerId);
                    drag.current = { px: e.clientX, py: e.clientY, x: pos.x, y: pos.y };
                  }}
                  onPointerMove={(e) => {
                    const d = drag.current;
                    if (!d) return;
                    setPos(clamp(d.x + (e.clientX - d.px) / scale, d.y + (e.clientY - d.py) / scale, size));
                  }}
                  onPointerUp={() => (drag.current = null)}
                  onPointerCancel={() => (drag.current = null)}
                />
              </div>
              <div className="flex items-center gap-3">
                <span className="text-xs text-muted">크기</span>
                <input
                  type="range"
                  min={minSize}
                  max={short}
                  value={size}
                  onChange={(e) => resize(Number(e.target.value))}
                  className="flex-1 accent-brand"
                  aria-label="선택 영역 크기"
                />
                <span className="w-16 text-right font-mono text-xs text-muted">{size}px</span>
              </div>
              <div className="flex gap-2">
                <Button size="sm" onClick={center}>
                  <LocateFixed className="size-4" /> 가운데로
                </Button>
                <Button
                  size="sm"
                  onClick={() => {
                    setSize(short);
                    setPos(clamp((width - short) / 2, (height - short) / 2, short));
                  }}
                >
                  <Maximize2 className="size-4" /> 최대 영역
                </Button>
              </div>
            </div>

            <div className="space-y-3">
              <p className="text-xs font-semibold uppercase tracking-wide text-muted">Discord에서 보이는 모습</p>
              <div className="flex items-end gap-4 rounded-xl bg-[#111214] p-4">
                {preview(112)}
                <div className="space-y-1 text-center">
                  {preview(28, true)}
                  <p className="text-[10px] text-muted">작은 이미지</p>
                </div>
              </div>
              <p className="text-xs text-muted">
                원본 {width}×{height} 중 {size}×{size}를 1024×1024로 만듭니다.
              </p>
            </div>
          </div>

          {size < 512 && (
            <Banner kind="warn">
              선택한 영역이 {size}px라서 1024px로 키우면 흐려집니다. 선명하게 하려면 더 큰 원본을 쓰세요.
            </Banner>
          )}

          {result ? (
            <Banner kind="ok">
              <p>
                <b>{result.fileName}</b> 을 만들었습니다 ({result.size}×{result.size}, {Math.round(result.bytes / 1024)}KB).
              </p>
              <p className="break-all text-xs text-muted">
                저장 위치: {result.path} · 설정 → 이미지에서 폴더를 바꿀 수 있습니다.
              </p>
              <ol className="list-decimal space-y-1 pl-5 text-sm">
                <li>
                  Developer Portal → 앱 → <b>Rich Presence → Art Assets</b>를 엽니다.
                </li>
                <li>
                  같은 이름의 기존 에셋이 있으면 삭제하고, 이 파일을 이름 <code>{result.fileName.replace(/\.png$/, "")}</code>
                  (에셋 키)으로 업로드합니다. 저장 버튼이 있으면 누르세요.
                </li>
                <li>앱에서 에셋 키 옆 <b>확인</b>을 누르면 바로 인식합니다.</li>
              </ol>
              <div className="flex gap-2 pt-1">
                <Button size="sm" onClick={() => void api.revealPath(result.path)}>
                  <FolderOpen className="size-4" /> 파일 위치 열기
                </Button>
                <Button size="sm" variant="ghost" onClick={() => window.open(PORTAL, "_blank")}>
                  Developer Portal 열기
                </Button>
              </div>
            </Banner>
          ) : (
            <div className="flex flex-wrap items-end gap-3 border-t border-line pt-4">
              <Field label="내보낼 에셋 이름 (키)" className="min-w-60 flex-1">
                <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="예: elden-ring" />
              </Field>
              <Button variant="primary" busy={busy} disabled={!name.trim()} onClick={() => void exportIt()}>
                1024×1024로 내보내기
              </Button>
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}
