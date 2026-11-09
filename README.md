# Console Presence

Nintendo Switch / Switch 2, PlayStation, Xbox 같은 **콘솔 게임을 플레이 중일 때 Discord 프로필에 표시**해 주는 데스크톱 앱입니다.
콘솔 게임은 PC에서 실행되는 프로세스가 없어 Discord의 자동 게임 감지에 잡히지 않습니다. 이 앱은 사용자가 게임을 직접 고르면,
Discord가 공식 문서화한 **Rich Presence RPC(로컬 IPC)** 로 게임 이름·썸네일·경과 시간을 보여줍니다.

- Discord 게임 감지를 변조하거나 가상 프로세스를 만들지 않습니다.
- 계정 토큰이나 비공식 인증을 쓰지 않습니다. 앱이 하는 일은 실행 중인 Discord 데스크톱 앱과의 로컬 통신뿐입니다.
- 스택: Tauri 2 · Rust · React 19 · TypeScript · Tailwind CSS v4. macOS와 Windows가 같은 코드베이스입니다.

> ⚠️ Rich Presence는 **공개 프로필에 표시**됩니다. 처음 시험할 때는 테스트 계정이나 비공개 서버 전용 계정을 쓰는 것을 Discord도 권장합니다.

## 현재 검증 상태 (먼저 읽어 주세요)

| 항목 | 상태 |
|---|---|
| Rust 테스트 129개 (저장소·세션·Presence 생성·IPC 워커·전송 타임아웃·이미지 캐시·다운로드·에셋 API·커버 검색·크롭) | ✅ 통과 (macOS arm64, 3회 연속) |
| `tsc` 타입 검사, `vite build` | ✅ 통과 |
| 실제 Tauri 앱 기동 (macOS, `tauri dev`) — 패닉 없음, 라이브러리 8종 시드, 세션 파일 하트비트 | ✅ 확인 |
| UI 렌더링 — 목(mock) 백엔드 + 헤드리스 Chromium으로 라이브러리·상세·플레이 중·설정 화면 확인 | ✅ 확인 (실제 Tauri 창의 스크린샷은 아님) |
| 실제 Discord 클라이언트와 IPC 핸드셰이크 — 존재하지 않는 Application ID는 `CLOSE 4000 "Invalid Client ID"`로 응답하고, 앱이 이를 재시도 불가 오류로 표시 | ✅ 확인 |
| 유효한 Application ID로 Presence 전송 — 실제 Discord가 수락하고 처리한 활동을 응답으로 돌려줌 | ✅ 확인 |
| 외부 이미지 URL — Discord가 `mp:external/…`로 변환했고, 미디어 프록시가 이미지를 실제로 가져옴 (HTTP 200, `image/jpeg`, 273×364) | ✅ 확인 |
| 타임스탬프 단위 — **초**로 보내면 Discord가 밀리초로 변환해 저장하고, 밀리초로 보내면 그대로 둠 (앱은 초를 보냄) | ✅ 확인 |
| 존재하지 않는 Asset Key — 실제 에셋 목록(공개 엔드포인트)과 대조해 경고하고 이미지를 보내지 않음 | ✅ 확인 |
| `name` 오버라이드 — 프로필 카드 맨 위에 앱 이름이 아니라 보낸 게임 이름이 표시됨 | ✅ 사용자 육안 확인 |
| **프로필 카드에서 실제로 보이는 모습** — 외부 URL 이미지가 표시되고, 경과 시간이 올라감 | ✅ 사용자 육안 확인 (macOS Discord, 2026-11-09). 타이머의 시작값이 정확히 1:30:00 근처인지는 따로 확인하지 않았습니다 |
| Discord 에셋 키 — 키가 에셋 ID로 해석되고, 큰 이미지와 작은 이미지가 프로필에 표시됨 (등록된 에셋 1개, 128×128) | ✅ 사용자 육안 확인 |
| Tauri 앱 창에서 Application ID를 입력해 연결하고 게임을 시작해 Discord에 표시 (`npm run tauri dev`, macOS) | ✅ 사용자가 직접 실행해 확인 |
| 게임을 바꿀 때의 이미지 교체 (에셋 2개: `pokemon-za` → `ocarina-of-time`) | ✅ 사용자가 직접 실행해 확인 |
| 크롭 내보내기 — 선택한 영역이 정확히 잘려 1024×1024 PNG로 저장됨 (단위 테스트 7개), 화면 동작은 목 백엔드 + 브라우저로 확인 | ✅ 확인 / ❌ 실제 앱 창에서 직접 써 보는 것은 미검증 |
| 실제 Discord가 응답하지 않을 때(요청 제한) 워커가 멈추지 않음 | ✅ 실제 Discord에서 결함을 발견해 수정·확인 |
| 실제 Discord를 껐다 켰을 때의 재연결·Presence 복원 (macOS) | ✅ 사용자가 직접 실행해 확인 |
| Windows 빌드 — GitHub Actions(`windows-latest`)에서 타입 검사·Rust 테스트·`tauri build`가 모두 통과하고 `.msi`와 `-setup.exe`가 만들어짐 | ✅ CI로 확인 (설치 파일을 받아 실행해 본 것은 아님) |
| Windows에서 앱 실행과 Discord 연결 (named pipe) | ❌ **미검증** — Windows PC에서 실행해 보지 못했습니다 |
| 로그인 시 자동 실행 (autostart) | ❌ 미검증 |

미검증 항목은 `cargo run --example live_check`로 직접 확인할 수 있습니다 ([아래](#실제-discord로-검증하기)).

## 빠른 시작

### 1. 준비물
- Node.js 22+ , npm
- Rust (stable) — <https://rustup.rs>. 설치 후 `. "$HOME/.cargo/env"`로 PATH를 잡거나 새 터미널을 여세요.
- **macOS:** Xcode Command Line Tools (`xcode-select --install`)
- **Windows:** [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)(“C++를 사용한 데스크톱 개발”), WebView2 (Windows 11 기본 포함)

### 2. 실행
```bash
npm install
npm run tauri dev
```

### 3. Discord Application ID 설정
1. [Discord Developer Portal](https://discord.com/developers/applications) → **New Application** → 이름을 정합니다(기본 표시 이름이 됩니다).
2. **General Information**의 **Application ID**(숫자)를 복사합니다.
3. 앱 → **설정 → Discord**에 붙여 넣고 **저장 후 연결**을 누릅니다. **연결 테스트**로 Discord 연결과 ID가 올바른지, 등록된 에셋이 몇 개인지 확인할 수 있습니다.

Discord 데스크톱 앱이 켜져 있어야 합니다. 나중에 켜거나 재시작해도 앱이 3초 간격으로 다시 연결하고 Presence를 복원합니다.

### 4. Discord 에셋(썸네일) 등록
1. Developer Portal → 앱 → **Rich Presence → Art Assets** → 이미지를 업로드합니다 (1024×1024 권장, 앱당 최대 300개).
2. **에셋 이름이 Asset Key**입니다. 포털이 소문자로 저장하므로 소문자로 쓰세요.
3. 앱에서 게임의 **Asset Key**에 같은 이름을 입력하고 **확인**을 누르면 등록 여부를 검사합니다. 기본 게임 8종은 `pokemon-za`, `mario-kart-world`, `zelda-totk`, `smash-ultimate`, `persona-5-royal`, `elden-ring`, `monster-hunter-wilds`, `cyberpunk-2077` 키를 기대합니다. 업로드하지 않았다면 기본 이미지로 대체되고 경고가 표시됩니다.
   방금 올린 에셋도 **확인**이 바로 인식합니다. Discord의 에셋 목록 주소는 CDN이 응답을 최대 10분 캐시해서(`s-maxage=600`) 일반 요청으로는 새 에셋이 한참 뒤에 보이는데, 앱은 캐시를 우회해서 조회합니다.

> 커버 이미지는 이 저장소에 포함하지 않습니다. 게임 커버는 저작물이므로 각자 가진 이미지를 직접 업로드하세요.

## 이미지 소스

게임 데이터와 이미지 데이터는 분리되어 있고, 소스는 서로 구분됩니다.

| 소스 | Discord에서 표시 | 설명 |
|---|---|---|
| **Discord 에셋** | ✅ | Developer Portal에 등록한 에셋 키. 앱이 등록 여부를 공개 엔드포인트로 검사하고, Discord CDN에서 미리보기를 받아옵니다. |
| **외부 URL** | ✅ (문서상) | 공개 https 이미지 주소를 `large_image` 값에 **그대로** 보냅니다. |
| **업로드** | ⚠️ 단독으로는 ✗ | 파일을 검증하고 1024px 이하로 줄여 앱 데이터 폴더에 보관합니다. 미리보기용입니다. |
| **로컬 파일** | ⚠️ 단독으로는 ✗ | 경로만 기억합니다. 미리보기용입니다. |
| **기본 이미지** | 설정에 따름 | 설정의 기본 Asset Key → 기본 URL → (둘 다 없으면) Discord 앱 아이콘 순서. |

### 외부 URL을 쓸 수 있나요?
**네, Discord 공식 문서가 지원을 명시합니다.** `Activity Asset Image`: *"To use an external image via media proxy, specify the URL as the field's value when sending"* — 임의 공개 이미지 URL이면 되고, 업로드 에셋과 달리 GIF·애니메이션 WebP·AVIF도 허용됩니다(“적절한 크기여야 한다”는 조건이 있습니다).
앱은 URL을 **임의 문자열로 Asset Key 자리에 넣지 않고**, 먼저 `http/https`·공개 호스트인지 검사한 뒤 보냅니다. 같은 URL은 앱이 내려받아(10MB 제한·이미지 시그니처 검사·손상 검사·재시도) 미리보기와 경고(저해상도/비정사각)에 씁니다.
실제 Discord로 확인했습니다: Discord가 URL을 `mp:external/…`로 변환했고, 그 미디어 프록시 주소를 요청하면 원본과 같은 이미지(273×364 JPEG)가 HTTP 200으로 옵니다. 프로필 카드에서도 이미지가 표시되는 것을 육안으로 확인했습니다.

### 로컬 이미지는 왜 Discord에 안 보이나요?
Discord의 미디어 프록시는 Discord 서버에서 이미지를 가져가므로 내 PC의 파일이나 `localhost`는 볼 수 없고, 에셋 업로드를 자동화하는 공식 API도 없습니다(토큰을 쓰는 비공식 방법은 사용하지 않습니다). 그래서 로컬 이미지는 ① Developer Portal에 직접 올리고 그 키를 입력하거나 ② 공개 URL로 호스팅해서 쓰도록 안내합니다. 이 한계를 숨기지 않고 UI에 경고로 표시합니다.

### 커버 이미지 찾기
외부 URL 소스에서 영어 위키백과 게임 문서의 표지를 검색합니다(키 불필요). 검색이 부정확해서(“Persona 5 Royal” → “Persona 5”, “Elden Ring” → 영화 문서) **후보 목록에서 직접 고르는 방식**이고, 표지는 250~270px 정도로 작아 경고가 표시됩니다.

## 주요 기능
- **라이브러리** — 검색, 플랫폼 필터, 즐겨찾기, 최근 플레이, 게임 추가·수정·삭제
- **Rich Presence 편집** — Game Name, Details, State, Large/Small Image와 텍스트, Start Timestamp, 실시간 Discord 카드 미리보기
- **세션** — 시작/종료, 경과 시간, 숨기기/다시 표시, 최근 기록, 중복 방지(한 번에 하나), 앱 재시작 시 복구, 자동 종료(시간 제한)
- **연결 분리** — 게임 세션과 Discord 연결은 별개입니다. Discord가 꺼져도 세션은 계속되고, 돌아오면 Presence가 복원됩니다.
- **이미지 캐시** — 내용 해시 파일명(중복 제거), URL 변경 시 갱신, 실패 재시도(5xx/429/네트워크), `ETag` 재검증, 크기 상한(LRU), 비우기
- **크롭 내보내기** — 이미지에서 Discord에 보일 정사각형 영역을 직접 골라 1024×1024 PNG로 내보냅니다 ([아래](#이미지-크롭-내보내기))
- **설정** — Application ID, 자동 연결, 로그인 시 실행, 마지막 게임 복원, 자동 초기화, 기본 상태, 기본 이미지

### 이미지 크롭 내보내기
Discord는 큰 이미지를 정사각형으로 보여 주고, 직사각형이면 가운데를 기준으로 알아서 자릅니다. 자르는 방식을 정하는 값은 없어서, 제어할 수 있는 것은 **Discord가 받는 이미지 자체**뿐입니다. 이미지 설정의 **크롭** 버튼에서 원하는 정사각형 영역을 끌어서 정하면(크기 슬라이더, Discord에 보일 큰 이미지·작은 이미지 미리보기) 그 부분을 1024×1024 PNG로 내보냅니다. 기본 저장 위치는 사진 폴더 안의 `Console Presence/<키>.png`(예: `~/Pictures/Console Presence`)이고, **설정 → 이미지**에서 폴더를 직접 고르거나 바로 열 수 있습니다.

Discord에는 에셋 업로드 API가 없고 앱은 계정 토큰을 쓰지 않으므로, **마지막 단계(Developer Portal에 같은 이름으로 업로드)는 직접** 해야 합니다. 업로드 뒤 **확인**을 누르면 바로 인식합니다. 선택 영역이 512px보다 작으면 키워도 선명해지지 않는다고 경고합니다. 화질 자체는 더 큰 원본으로만 좋아집니다.

### Discord 쪽 동작에 대한 메모
- **‘방송 중’ 표시는 만들지 않았습니다:** Discord는 앱이 보내는 ‘스트리밍’ 활동을 거부합니다(실제 응답: `"type" must be one of [0, 2, 3, 5]`). 화면 공유(Go Live)의 `LIVE` 표시와 공유 중인 프로그램 이름(예: OBS)은 Discord 클라이언트가 직접 만들어서 앱이 바꾸거나 읽을 수 없습니다.
- **게임 이름 표시:** 카드 맨 위 이름은 기본적으로 앱 이름이고, 활동의 `name` 필드로 바꿀 수 있다고 Social SDK 문서(2026-06-05 추가)에 나옵니다. 설정의 *“Discord 카드에 게임 이름으로 표시”*(기본 켜짐)가 이를 사용합니다. 실제 Discord에서 게임 이름으로 표시되는 것을 확인했습니다. 앱 이름을 쓰고 싶으면 끄세요.
- **타임스탬프 단위:** Social SDK 가이드는 *초*, Gateway Activity 표는 *밀리초*라고 적혀 있어 서로 달랐습니다. 실제 Discord에서 확인한 결과, IPC로는 *초*를 보내면 Discord가 *밀리초*(×1000)로 변환해 저장하고 밀리초는 그대로 둡니다. 즉 문서의 두 표현은 입력(초)과 저장 형태(밀리초)의 차이였습니다. 앱은 *초*를 보냅니다.
- **요청 제한:** 같은 Application ID로 연결이나 업데이트를 짧은 시간에 반복하면 Discord가 **응답 없이 무시**하고(핸드셰이크 포함), 1~2분 뒤 정상으로 돌아옵니다. 앱은 응답 대기에 타임아웃을 두고, 응답이 없으면 연결을 끊지 않고 "미확인"으로 표시한 뒤 15초 하트비트로 다시 보내며, 핸드셰이크가 무시되면 재시도 간격을 5배로 늘립니다. **개발 중 `tauri dev`나 `live_check`를 몇 초 간격으로 반복 실행하면 이 제한에 걸려 Presence가 안 뜰 수 있으니** 몇 분 쉬었다가 다시 시도하세요.
- **업데이트 빈도:** Discord는 20초에 5회까지만 허용하므로 앱은 20초당 4회 이하로 제한하고, 15초마다 같은 Presence를 다시 보내 연결이 끊겼는지 확인합니다.
- **Discord 응답 확인:** 사용한 라이브러리는 전송만 하고 응답을 읽지 않으므로, 앱이 응답을 직접 읽어 거부된 Presence와 끊어진 연결을 감지합니다.

## RPC 라이브러리 선정
| | `discord-rich-presence` 1.1.0 (**선정**) | `discord-presence` 3.2.0 | `discord-sdk` 0.4.0 |
|---|---|---|---|
| 성격 | 얇은 IPC 클라이언트 | 스레드·이벤트·재연결 내장 | Game SDK 구현 |
| 최신 Activity 필드(`name`, `status_display_type`, `*_url`) | ✅ | `name`은 `unstable_name` 기능 뒤 | — |
| 이미지 필드 | 문자열(에셋 키 **또는 URL**) | 문자열 | — |
| 제약 | 핸드셰이크 응답 폐기, 전송 후 응답 미독, 읽기 타임아웃 없음 | `READY`가 프로세스 전역 static이라 앱 ID 교체·재시작과 맞지 않음 | 용도가 다름 |

선정 이유: 최신 필드를 지원하고 `recv()`가 공개라서 부족한 부분(핸드셰이크 응답, 응답 확인)을 앱이 직접 처리할 수 있습니다. 라이브러리가 지원하는 것과 Discord가 문서화한 것은 구분해서 다뤘습니다. 예: 라이브러리 문서의 “asset name or URL”은 라이브러리의 주장이고, URL 지원의 근거는 Discord 공식 문서입니다.

## 빌드

### macOS
```bash
npm install
npm run tauri build                  # 모든 번들 (.app, .dmg)
npm run tauri build -- --bundles app # .app만
```
결과물 (Apple Silicon에서 확인):
- `src-tauri/target/release/bundle/macos/Console Presence.app`
- `src-tauri/target/release/bundle/dmg/Console Presence_0.1.0_aarch64.dmg`

Intel Mac용은 그 Mac에서 빌드하세요. 서명·공증을 하지 않았으므로, 다른 곳에서 내려받은 `.app`은 처음 열 때 차단될 수 있습니다 (*우클릭 → 열기*, 또는 `xattr -dr com.apple.quarantine "/Applications/Console Presence.app"`).

### Windows
Windows PC에서:
```powershell
npm install
npm run tauri build
```
결과물: `src-tauri\target\release\bundle\` 아래 `.msi`(WiX) / `-setup.exe`(NSIS). 크로스 컴파일은 지원하지 않으니 Windows 머신이나 CI가 필요합니다. `.github/workflows/build.yml`이 push마다 macOS·Windows 러너에서 타입 검사·Rust 테스트·번들 빌드를 실행하고 설치 파일을 **아티팩트**로 올립니다. GitHub 저장소 → **Actions** → 실행 선택 → 맨 아래 **Artifacts**에서 내려받을 수 있습니다 (`console-presence-windows-latest` 안에 `nsis/…-setup.exe`와 `msi/….msi`, 약 90일 보관). 아티팩트는 *Release*가 아니라서 Releases 탭에는 나타나지 않습니다.
서명하지 않은 설치 파일이라 Windows SmartScreen이 "알 수 없는 게시자" 경고를 띄울 수 있습니다(*추가 정보 → 실행*).
TLS는 두 OS 모두 시스템 구현(`native-tls`: Security.framework / SChannel)을 써서 별도 C 빌드 도구가 필요 없습니다.

## 개발

```bash
npm run dev          # 프런트만 (브라우저). Tauri가 없으면 개발용 목 백엔드가 자동으로 켜집니다.
npm run tauri dev    # 앱 전체
npx tsc --noEmit     # 타입 검사
cd src-tauri && cargo test   # Rust 테스트 (129개)
```

### 실제 Discord로 검증하기
Discord를 켜 두고(테스트 Presence가 프로필에 잠시 표시됩니다):
```bash
cd src-tauri
cargo run --example live_check -- <APPLICATION_ID> --image pokemon-za --ago 90 --hold 60
cargo run --example live_check -- <APPLICATION_ID> --image https://example.com/cover.png
cargo run --example live_check -- <APPLICATION_ID> --raw s     # 타이머가 1:30:00 근처면 '초'가 맞습니다
cargo run --example live_check -- <APPLICATION_ID> --raw ms    # 이쪽이 맞다면 알려주세요
```

### 구조
```
src/                       React 앱
  features/game-library/   라이브러리, 카드, 게임 추가·수정
  features/presence/       Rich Presence 편집, 카드 미리보기, 게임 상세
  features/session/        현재 플레이 중
  features/image-manager/  이미지 소스 선택, 미리보기, 커버 찾기
  features/settings/       설정
  hooks/ lib/ types/       상태, Tauri API 래퍼, 타입 (Rust 모델 미러)
src-tauri/src/
  discord/                 IPC 워커(연결·재연결·하트비트), Presence 생성
  session/                 세션 로직 (Discord와 독립)
  storage/                 원자적 JSON 저장소, 모델, 기본 게임
  image/                   캐시, 다운로드, 에셋 조회, 커버 검색, 로컬 가져오기
  commands/                Tauri 커맨드
  state.rs                 공유 상태 (세션 ↔ Discord 연결은 sync_presence 한 곳)
src-tauri/examples/live_check.rs   실제 Discord 검증 도구
```

### 데이터 위치
- 사용자 데이터: `settings.json`, `library.json`, `session.json`, `uploads/` — macOS `~/Library/Application Support/com.consolepresence.app/`, Windows `%APPDATA%\com.consolepresence.app\`
- 캐시(언제든 지워도 됨): macOS `~/Library/Caches/com.consolepresence.app/images/`, Windows `%LOCALAPPDATA%\com.consolepresence.app\`
- 설정 저장은 Tauri Plugin Store 대신 Rust 쪽 원자적 JSON 저장소를 씁니다. 앱 시작 시 프런트 없이도 세션을 복구해야 하고, 저장 파일이 깨지면 `.corrupt-<시각>`으로 옮겨 보존하기 위해서입니다.

## 알려진 한계
- 앱을 종료하면 Presence가 사라집니다(Discord가 연결 종료 시 지움). 트레이 상주는 아직 없습니다.
- 응답 타임아웃은 macOS(Unix 소켓)에만 적용됩니다. Windows는 표준 라이브러리만으로 named pipe 읽기에 타임아웃을 걸 수 없어 `discord-rich-presence`의 기본 클라이언트를 그대로 쓰므로, Discord가 응답하지 않으면 Windows에서는 워커가 멈출 수 있습니다 (Windows는 어차피 미검증).
- 이미지 URL 검사는 호스트 이름·IP 리터럴·리다이렉트 대상까지만 확인합니다. 이름이 사설 IP로 해석되는 경우(DNS 리바인딩)는 막지 않습니다.
- 커버 검색은 영어 위키백과 의존이라 최신작·비주류 게임은 결과가 없을 수 있습니다.
- Presence 이미지가 실제로 그려졌는지는 Discord가 알려주지 않으므로, 앱은 *보낸 내용*과 *대체한 이유*만 보고합니다.
- 앱 아이콘은 Tauri 기본값입니다 (`npm run tauri icon <이미지>`로 교체).
