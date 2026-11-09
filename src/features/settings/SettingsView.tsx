import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, ExternalLink, FlaskConical, FolderOpen, Plug, Trash2, Unplug } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Banner, Button, Field, Input, Section, Select, Toggle } from "../../components/ui";
import { connectionLabel, useApp } from "../../hooks/AppContext";
import { api } from "../../lib/api";
import { formatBytes, timeAgo } from "../../lib/format";
import type { AppInfo, CacheOverview, ConnectionTest, ImageSource, Settings } from "../../types";

const PORTAL = "https://discord.com/developers/applications";

// Edits that were not saved when the user left this screen. Going back and
// forth between the library and the settings must not silently throw them away.
let unsaved: { base: Settings; draft: Settings } | null = null;

export function SettingsView({ onBack }: { onBack: () => void }) {
  const { settings, saveSettings, connect, disconnect, discord, toast, bumpImages } = useApp();
  const [draft, setDraft] = useState<Settings>(() =>
    unsaved && JSON.stringify(unsaved.base) === JSON.stringify(settings) ? unsaved.draft : settings,
  );
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  useEffect(
    () => () => {
      const changed = JSON.stringify(draftRef.current) !== JSON.stringify(settingsRef.current);
      unsaved = changed ? { base: settingsRef.current, draft: draftRef.current } : null;
    },
    [],
  );
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<ConnectionTest | null>(null);
  const [cache, setCache] = useState<CacheOverview | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);

  // Follow the saved settings, but do not reset a draft restored on mount.
  const lastSettings = useRef(settings);
  useEffect(() => {
    if (lastSettings.current !== settings) {
      lastSettings.current = settings;
      setDraft(settings);
    }
  }, [settings]);

  const refreshCache = useCallback(
    () => api.getImageCache().then(setCache).catch(() => undefined),
    [],
  );
  useEffect(() => {
    void refreshCache();
    api.getAppInfo().then(setInfo).catch(() => undefined);
  }, [refreshCache]);

  const set = (changes: Partial<Settings>) => setDraft((d) => ({ ...d, ...changes }));
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);
  const connected = discord.state === "connected";

  const runTest = async () => {
    setTesting(true);
    try {
      setTest(await api.testDiscordConnection(draft.applicationId));
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="mx-auto max-w-3xl space-y-5 pb-24">
      <div className="flex items-center gap-3">
        <Button onClick={onBack}>
          <ArrowLeft className="size-4" /> 라이브러리로 돌아가기
        </Button>
        <h1 className="text-xl font-bold">설정</h1>
      </div>

      <Section
        title="Discord"
        description="Rich Presence는 실행 중인 Discord 데스크톱 앱과 로컬 IPC로 통신합니다. 계정 토큰은 사용하지 않습니다."
        action={
          <span
            className={`rounded-full px-2.5 py-1 text-xs font-semibold ${
              connected ? "bg-ok/20 text-ok" : discord.state === "error" ? "bg-bad/20 text-bad" : "bg-line text-muted"
            }`}
          >
            {connectionLabel(discord.state)}
            {discord.user && ` · ${discord.user.globalName ?? discord.user.username}`}
          </span>
        }
      >
        <Field
          label="Application ID"
          hint="Developer Portal → 앱 선택 → General Information → Application ID (숫자)"
        >
          <div className="flex flex-wrap gap-2">
            <Input
              className="min-w-64 flex-1 font-mono"
              value={draft.applicationId}
              onChange={(e) => set({ applicationId: e.target.value })}
              placeholder="예: 123456789012345678"
              inputMode="numeric"
              spellCheck={false}
            />
            <Button
              variant="primary"
              disabled={!draft.applicationId.trim()}
              onClick={() => void connect(draft.applicationId)}
            >
              <Plug className="size-4" /> 저장 후 연결
            </Button>
            <Button busy={testing} disabled={!draft.applicationId.trim()} onClick={() => void runTest()}>
              <FlaskConical className="size-4" /> 연결 테스트
            </Button>
            {discord.state !== "idle" && (
              <Button variant="ghost" onClick={() => void disconnect()}>
                <Unplug className="size-4" /> 연결 해제
              </Button>
            )}
          </div>
        </Field>

        {discord.message && discord.state !== "connected" && (
          <Banner kind={discord.state === "error" ? "error" : "warn"}>{discord.message}</Banner>
        )}
        {test && (
          <Banner kind={test.ok ? "ok" : "error"}>
            <p>{test.message}</p>
            {test.ok && test.assetCount !== null && (
              <p className="text-xs text-muted">이 앱에 등록된 Rich Presence 에셋: {test.assetCount}개</p>
            )}
            {test.ok && test.assetError && (
              <p className="text-xs text-warn">에셋 목록을 읽지 못했습니다: {test.assetError}</p>
            )}
          </Banner>
        )}

        <Toggle
          checked={draft.autoConnect}
          onChange={(autoConnect) => set({ autoConnect })}
          label="앱 시작 시 자동으로 연결"
          hint="Discord가 나중에 실행되거나 재시작돼도 자동으로 다시 연결되고 Presence가 복원됩니다."
        />

        <details className="rounded-lg border border-line bg-bg/60 p-4 text-sm">
          <summary className="cursor-pointer font-medium">처음 설정하는 방법 (Application ID · 에셋 등록)</summary>
          <ol className="mt-3 list-decimal space-y-1.5 pl-5 text-muted">
            <li>
              <button className="text-brand underline" onClick={() => void openUrl(PORTAL)}>
                Discord Developer Portal <ExternalLink className="inline size-3" />
              </button>
              에서 <b>New Application</b>으로 앱을 만듭니다. 앱 이름이 기본 표시 이름이 됩니다.
            </li>
            <li>General Information의 <b>Application ID</b>를 위 칸에 붙여 넣고 <b>저장 후 연결</b>을 누릅니다.</li>
            <li>
              게임 썸네일을 에셋으로 쓰려면 <b>Rich Presence → Art Assets</b>에서 이미지를 올리고 게임에 지정한 키(예:{" "}
              <code>pokemon-za</code>)와 같은 이름을 붙입니다. 1024×1024 권장, 최대 300개입니다.
            </li>
            <li>에셋 대신 공개 이미지 URL을 쓸 수도 있습니다 (게임 상세 → 이미지 설정 → 외부 URL).</li>
          </ol>
        </details>
      </Section>

      <Section title="동작">
        <Toggle
          checked={draft.launchAtLogin}
          onChange={(launchAtLogin) => set({ launchAtLogin })}
          label="로그인 시 앱 자동 실행"
        />
        <Toggle
          checked={draft.restoreLastSession}
          onChange={(restoreLastSession) => set({ restoreLastSession })}
          label="마지막 게임 복원"
          hint="앱이 종료되거나 중단됐을 때 진행 중이던 세션을 다음 실행에서 이어서 표시합니다."
        />
        <Toggle
          checked={draft.overrideActivityName}
          onChange={(overrideActivityName) => set({ overrideActivityName })}
          label="Discord 카드에 게임 이름으로 표시"
          hint="끄면 Developer Portal의 앱 이름이 표시됩니다."
        />
        <Field
          label="Presence 자동 초기화 (시간)"
          hint="세션이 이 시간을 넘기면 자동으로 종료하고 Presence를 지웁니다. 0이면 사용하지 않습니다."
        >
          <Input
            type="number"
            min={0}
            max={168}
            className="w-32"
            value={draft.autoClearHours}
            onChange={(e) => set({ autoClearHours: Math.max(0, Number(e.target.value) || 0) })}
          />
        </Field>
      </Section>

      <Section title="기본 게임 상태" description="게임에 Details/State가 비어 있을 때 사용합니다.">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Details">
            <Input value={draft.defaultDetails} maxLength={128} onChange={(e) => set({ defaultDetails: e.target.value })} />
          </Field>
          <Field label="State">
            <Input value={draft.defaultState} maxLength={128} onChange={(e) => set({ defaultState: e.target.value })} />
          </Field>
        </div>
      </Section>

      <Section
        title="이미지"
        description="이미지가 없거나 사용할 수 없을 때 보여줄 기본 이미지와 캐시를 관리합니다."
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="새 게임의 기본 이미지 소스">
            <Select
              value={draft.defaultImageSource}
              onChange={(e) => set({ defaultImageSource: e.target.value as ImageSource })}
            >
              <option value="discord_asset">Discord 에셋</option>
              <option value="external_url">외부 URL</option>
              <option value="uploaded">업로드</option>
              <option value="local_file">로컬 파일</option>
              <option value="default">기본 이미지</option>
            </Select>
          </Field>
          <Field label="이미지 캐시 최대 크기 (MB)">
            <Input
              type="number"
              min={16}
              max={2048}
              value={draft.imageCacheMaxMb}
              onChange={(e) => set({ imageCacheMaxMb: Number(e.target.value) || 16 })}
            />
          </Field>
          <Field label="기본 이미지 · Asset Key" hint="우선 사용됩니다. 비워두면 Discord 앱 아이콘이 표시됩니다.">
            <Input
              value={draft.fallbackAssetKey}
              onChange={(e) => set({ fallbackAssetKey: e.target.value })}
              placeholder="예: console-default"
            />
          </Field>
          <Field label="기본 이미지 · URL" hint="Asset Key가 없을 때 사용하는 공개 https 주소">
            <Input
              value={draft.fallbackImageUrl}
              onChange={(e) => set({ fallbackImageUrl: e.target.value })}
              placeholder="https://…"
            />
          </Field>
        </div>

        <Field
          label="크롭 내보내기 폴더"
          hint="크롭한 이미지를 저장하는 곳입니다. 기본값은 사진 폴더 안의 Console Presence입니다."
        >
          <div className="flex flex-wrap gap-2">
            <Input
              readOnly
              value={draft.exportDir || info?.defaultExportDir || ""}
              className="min-w-64 flex-1 font-mono text-xs"
              aria-label="크롭 내보내기 폴더"
            />
            <Button
              onClick={async () => {
                const picked = await open({
                  directory: true,
                  multiple: false,
                  defaultPath: draft.exportDir || info?.defaultExportDir,
                });
                if (typeof picked === "string") set({ exportDir: picked });
              }}
            >
              폴더 선택…
            </Button>
            {draft.exportDir && (
              <Button variant="ghost" onClick={() => set({ exportDir: "" })}>
                기본값으로
              </Button>
            )}
            <Button
              variant="ghost"
              disabled={draft.exportDir !== settings.exportDir}
              title={draft.exportDir !== settings.exportDir ? "먼저 설정을 저장하세요" : undefined}
              onClick={() =>
                void api
                  .openExportDir()
                  .catch((e: unknown) => toast(e instanceof Error ? e.message : String(e), "error"))
              }
            >
              <FolderOpen className="size-4" /> 열기
            </Button>
          </div>
        </Field>

        <div className="rounded-lg border border-line bg-bg/60 p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="text-sm">
              <span className="font-medium">이미지 캐시</span>{" "}
              <span className="text-muted">
                {cache
                  ? `${cache.stats.entries}개 · ${formatBytes(cache.stats.bytes)} / ${formatBytes(cache.stats.maxBytes)}`
                  : "—"}
              </span>
            </div>
            <Button
              size="sm"
              disabled={!cache?.stats.entries}
              onClick={async () => {
                const stats = await api.clearImageCache().catch((e: unknown) => {
                  toast(e instanceof Error ? e.message : String(e), "error");
                  return null;
                });
                if (stats) {
                  toast("이미지 캐시를 비웠습니다. 필요한 이미지는 다시 내려받습니다.", "ok");
                  bumpImages();
                  void refreshCache();
                }
              }}
            >
              <Trash2 className="size-4" /> 캐시 비우기
            </Button>
          </div>
          {cache && cache.entries.length > 0 && (
            <ul className="mt-3 max-h-40 space-y-1 overflow-y-auto text-xs text-muted">
              {cache.entries.slice(0, 20).map((e) => (
                <li key={e.key} className="flex justify-between gap-3">
                  <span className="truncate">{e.url.startsWith("local:") ? "로컬 파일 썸네일" : e.url}</span>
                  <span className="shrink-0">
                    {e.width && e.height ? `${e.width}×${e.height} · ` : ""}
                    {formatBytes(e.bytes)} · {timeAgo(e.lastUsedAt, Date.now())}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Section>

      {info && (
        <p className="text-center text-xs text-muted">
          Console Presence {info.version} · {info.os} · 데이터 {info.dataDir}
        </p>
      )}

      <div className="fixed inset-x-0 bottom-0 z-40 border-t border-line bg-panel/95 px-6 py-3 backdrop-blur">
        <div className="mx-auto flex max-w-3xl items-center justify-between gap-3">
          <span className="text-sm text-muted">{dirty ? "저장하지 않은 변경사항이 있습니다." : "모든 변경사항이 저장되었습니다."}</span>
          <div className="flex gap-2">
            <Button variant="ghost" disabled={!dirty} onClick={() => setDraft(settings)}>
              되돌리기
            </Button>
            <Button
              variant="primary"
              disabled={!dirty}
              busy={saving}
              onClick={async () => {
                setSaving(true);
                await saveSettings(draft);
                setSaving(false);
                void refreshCache();
              }}
            >
              설정 저장
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
