import { useState } from "react";

import { Button, Field, Input, Modal, Select } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { PLATFORMS } from "../../lib/platforms";
import type { Game, ImageSpec } from "../../types";
import { ImagePicker } from "../image-manager/ImagePicker";

const specOf = (game: Partial<Game>): ImageSpec => ({
  source: game.imageSource ?? "default",
  assetKey: game.assetKey ?? null,
  url: game.imageUrl ?? null,
  localPath: game.localImagePath ?? null,
});

/** Adds a game, or edits one when `game` is given. */
export function GameFormModal({
  game,
  onClose,
  onSaved,
  onDeleted,
}: {
  game?: Game;
  onClose: () => void;
  onSaved?: (game: Game) => void;
  onDeleted?: () => void;
}) {
  const { settings, addGame, updateGame, removeGame, session } = useApp();
  const editing = !!game;
  const [draft, setDraft] = useState<Partial<Game>>(
    () =>
      game ?? {
        name: "",
        platform: PLATFORMS[0],
        details: "",
        state: "",
        imageSource: settings.defaultImageSource,
        favorite: false,
      },
  );
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const set = (changes: Partial<Game>) => setDraft((d) => ({ ...d, ...changes }));
  const playing = editing && session?.gameId === game.id;
  const valid = (draft.name ?? "").trim().length > 0;

  const save = async () => {
    setSaving(true);
    const saved = editing ? await updateGame({ ...game, ...draft } as Game) : await addGame(draft);
    setSaving(false);
    if (saved) {
      onSaved?.(saved);
      onClose();
    }
  };

  return (
    <Modal title={editing ? "게임 수정" : "게임 추가"} onClose={onClose} wide>
      <div className="space-y-5">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="게임 이름" className="sm:col-span-2">
            <Input
              value={draft.name ?? ""}
              onChange={(e) => set({ name: e.target.value })}
              placeholder="예: Elden Ring"
              autoFocus
            />
          </Field>
          <Field label="플랫폼">
            <Select value={draft.platform ?? ""} onChange={(e) => set({ platform: e.target.value })}>
              {PLATFORMS.map((p) => (
                <option key={p}>{p}</option>
              ))}
            </Select>
          </Field>
          <span />
          <Field label="기본 Details" hint="지금 하고 있는 것 (예: Story Progress)">
            <Input value={draft.details ?? ""} onChange={(e) => set({ details: e.target.value })} maxLength={128} />
          </Field>
          <Field label="기본 State" hint="위치·상태 (예: Lumiose City)">
            <Input value={draft.state ?? ""} onChange={(e) => set({ state: e.target.value })} maxLength={128} />
          </Field>
        </div>

        <ImagePicker
          label="대표 썸네일 (Large Image)"
          value={specOf(draft)}
          gameName={draft.name || "게임"}
          allowCover
          onChange={(spec) =>
            set({
              imageSource: spec.source,
              assetKey: spec.assetKey ?? null,
              imageUrl: spec.url ?? null,
              localImagePath: spec.localPath ?? null,
            })
          }
        />

        <details className="rounded-lg border border-line bg-bg/60 p-4">
          <summary className="cursor-pointer text-sm font-medium">고급: 작은 이미지와 툴팁</summary>
          <div className="mt-4 space-y-4">
            <Field label="Large Image Text" hint="큰 이미지에 마우스를 올렸을 때 보이는 글. 비우면 '게임 · 플랫폼'">
              <Input
                value={draft.largeImageText ?? ""}
                onChange={(e) => set({ largeImageText: e.target.value })}
                maxLength={128}
              />
            </Field>
            <ImagePicker
              label="작은 이미지 (Small Image)"
              value={draft.smallImage ?? { source: "default" }}
              gameName={draft.name || "게임"}
              optional
              onChange={(spec) => set({ smallImage: spec.source === "default" ? null : spec })}
            />
            <Field label="Small Image Text" hint="비우면 플랫폼 이름">
              <Input
                value={draft.smallImageText ?? ""}
                onChange={(e) => set({ smallImageText: e.target.value })}
                maxLength={128}
              />
            </Field>
          </div>
        </details>

        <div className="flex items-center justify-between gap-3 border-t border-line pt-4">
          <div>
            {editing &&
              (confirmDelete ? (
                <span className="flex items-center gap-2">
                  <Button
                    variant="danger"
                    size="sm"
                    onClick={async () => {
                      if (await removeGame(game.id)) {
                        onDeleted?.();
                        onClose();
                      }
                    }}
                  >
                    정말 삭제
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => setConfirmDelete(false)}>
                    취소
                  </Button>
                </span>
              ) : (
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={playing}
                  title={playing ? "플레이 중인 게임은 삭제할 수 없습니다" : undefined}
                  onClick={() => setConfirmDelete(true)}
                >
                  삭제
                </Button>
              ))}
          </div>
          <div className="flex gap-2">
            <Button variant="ghost" onClick={onClose}>
              취소
            </Button>
            <Button variant="primary" busy={saving} disabled={!valid} onClick={() => void save()}>
              {editing ? "저장" : "추가"}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
