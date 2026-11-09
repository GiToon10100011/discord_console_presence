import { Button, Field, Input, Section, Toggle } from "../../components/ui";
import { useApp } from "../../hooks/AppContext";
import { fromLocalInput, toLocalInput } from "../../lib/format";
import type { PresenceConfig } from "../../types";
import { ImagePicker } from "../image-manager/ImagePicker";

const HOUR = 3_600_000;

/** The Rich Presence fields and the image settings of one presence. */
export function PresenceEditor({
  config,
  onChange,
  sessionStart,
}: {
  config: PresenceConfig;
  onChange: (next: PresenceConfig) => void;
  /** Start of the running session, shown when no custom start is set. */
  sessionStart: number | null;
}) {
  const { settings } = useApp();
  const set = (changes: Partial<PresenceConfig>) => onChange({ ...config, ...changes });
  const effectiveStart = config.startTimestamp ?? sessionStart;

  return (
    <div className="space-y-5">
      <Section title="Rich Presence" description="Discord 프로필 카드에 표시되는 글입니다.">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label="Game Name"
            className="sm:col-span-2"
            hint="카드 맨 위의 이름. 설정에서 '게임 이름으로 표시'를 켜면 앱 이름 대신 쓰입니다."
          >
            <Input value={config.gameName} onChange={(e) => set({ gameName: e.target.value })} maxLength={128} />
          </Field>
          <Field label="Details" hint="지금 하고 있는 것">
            <Input value={config.details} onChange={(e) => set({ details: e.target.value })} maxLength={128} />
          </Field>
          <Field label="State" hint="위치나 상태">
            <Input value={config.state} onChange={(e) => set({ state: e.target.value })} maxLength={128} />
          </Field>
          <Field label="Large Image Text" hint="큰 이미지에 마우스를 올렸을 때의 글 (플랫폼 표시에 좋습니다)">
            <Input
              value={config.largeImageText}
              onChange={(e) => set({ largeImageText: e.target.value })}
              maxLength={128}
            />
          </Field>
          <Field label="Small Image Text" hint="작은 이미지가 있을 때만 표시됩니다">
            <Input
              value={config.smallImageText}
              onChange={(e) => set({ smallImageText: e.target.value })}
              maxLength={128}
            />
          </Field>
          <Field
            label="Start Timestamp"
            className="sm:col-span-2"
            hint="경과 시간의 기준 시각. 이미 한참 하고 있었다면 과거 시각으로 바꾸세요. 비우면 플레이 시작 시각입니다."
          >
            <div className="flex flex-wrap items-center gap-2">
              <Input
                type="datetime-local"
                className="w-auto"
                value={toLocalInput(config.startTimestamp)}
                max={toLocalInput(Date.now())}
                onChange={(e) => set({ startTimestamp: fromLocalInput(e.target.value) })}
              />
              {[1, 2, 3].map((h) => (
                <Button key={h} size="sm" onClick={() => set({ startTimestamp: Date.now() - h * HOUR })}>
                  {h}시간 전
                </Button>
              ))}
              <Button size="sm" variant="ghost" onClick={() => set({ startTimestamp: null })}>
                {sessionStart || effectiveStart ? "세션 시작 시각으로" : "기본값"}
              </Button>
            </div>
          </Field>
        </div>
        <Toggle
          checked={config.live}
          onChange={(live) => set({ live })}
          label="방송 중으로 표시"
          hint={
            settings.streamUrl
              ? "상태 줄에 '🔴 방송 중'을 붙이고, 그 글자와 큰 이미지를 방송 주소로 연결하며, 버튼을 추가합니다 (버튼은 다른 사람에게만 보입니다)."
              : "방송 주소가 비어 있어 문구만 표시됩니다. 설정 → 방송에서 주소를 입력하면 링크와 버튼이 붙습니다."
          }
        />
      </Section>

      <Section
        title="이미지 설정"
        description="이미지 소스는 서로 다릅니다. Discord가 직접 보여줄 수 있는 것은 등록된 에셋 키와 공개 URL입니다."
      >
        <ImagePicker
          label="큰 이미지 (Large Image)"
          value={config.largeImage}
          gameName={config.gameName}
          allowCover
          onChange={(largeImage) => set({ largeImage })}
        />
        <div className="border-t border-line pt-4">
          <ImagePicker
            label="작은 이미지 (Small Image)"
            value={config.smallImage}
            gameName={config.gameName}
            optional
            onChange={(smallImage) => set({ smallImage })}
          />
        </div>
      </Section>
    </div>
  );
}
