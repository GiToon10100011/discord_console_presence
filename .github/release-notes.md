Nintendo Switch / PlayStation / Xbox 같은 **콘솔 게임을 플레이 중일 때 Discord 프로필에 표시**해 주는 데스크톱 앱입니다. Discord가 공식 문서화한 Rich Presence(로컬 IPC)만 쓰고, 계정 토큰은 쓰지 않습니다.

## 설치

| OS | 받을 파일 |
|---|---|
| macOS (Apple Silicon) | `.dmg` |
| Windows (x64) | `-setup.exe` (또는 `.msi`) |

설치 파일은 **Apple·Microsoft 개발자 서명과 공증을 하지 않았습니다.** (macOS 앱은 임시 서명만 되어 있습니다.) 처음 열 때 경고가 나옵니다.

- **macOS** — 앱을 `응용 프로그램`으로 옮기고 한 번 열어 본 뒤, *시스템 설정 → 개인정보 보호 및 보안 → 그래도 열기*를 누르세요. 또는 터미널에서 한 번 실행해도 됩니다.
  `xattr -dr com.apple.quarantine "/Applications/Console Presence.app"`
- **Windows** — SmartScreen이 "알 수 없는 게시자"라고 하면 *추가 정보 → 실행*.

Intel Mac용 빌드는 포함되어 있지 않습니다.

## 시작하기

1. Discord **데스크톱 앱**을 켭니다.
2. [Discord Developer Portal](https://discord.com/developers/applications)에서 앱을 만들고 **Application ID**를 복사합니다.
3. Console Presence → **설정**에 붙여 넣고 **저장 후 연결**을 누릅니다.
4. 라이브러리에서 게임을 골라 **시작**합니다.

썸네일은 Developer Portal의 **Rich Presence → Art Assets**에 올린 이미지의 이름을 게임의 *Asset Key*로 입력하거나, 공개 이미지 URL을 쓰면 됩니다. 앱의 **크롭** 기능으로 Discord에 보일 정사각형 영역을 골라 PNG로 내보낼 수 있습니다 (포털 업로드는 직접 해야 합니다).

## 무엇을 확인했나

macOS와 Windows에서 실제 Discord 클라이언트로 직접 실행해 확인한 항목과, 단위 테스트로만 확인한 항목은 [README의 검증 상태 표](https://github.com/GiToon10100011/discord_console_presence#현재-검증-상태-먼저-읽어-주세요)에 정리되어 있습니다. 확인하지 못한 항목(앱 재시작 후 세션 복원·자동 종료의 실제 동작, Windows에서 Discord가 응답하지 않을 때의 동작)도 거기에 그대로 적혀 있습니다.

## 알려진 한계

- 앱을 종료하면 Presence가 사라집니다 (트레이 상주는 아직 없습니다).
- Discord의 ‘방송 중’ 표시는 만들 수 없습니다. Discord가 앱의 스트리밍 활동을 거부합니다.
- 같은 Application ID로 짧은 시간에 연결을 반복하면 Discord가 응답을 무시해 Presence가 잠시 안 뜰 수 있습니다 (1~2분 뒤 복구).
