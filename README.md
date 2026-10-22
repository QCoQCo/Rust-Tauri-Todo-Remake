<p align="center">
  <strong>🌐 언어 선택 / 言語選択</strong>
</p>
<p align="center">
  <a href="#ko"><code>한국어</code></a>
  &nbsp;·&nbsp;
  <a href="#ja"><code>日本語</code></a>
</p>

---

<span id="ko"></span>

## 🇰🇷 한국어

# Todo App (Tauri + Vanilla)

Tauri와 Vanilla JavaScript로 만든 **데스크톱 투두·스탑워치·통계 앱**입니다. 할 일 관리, 스탑워치(랩 타임), 기간별 통계·차트, 암호화 백업/복원을 지원합니다.

---

### 주요 기능

| 기능 | 설명 |
|------|------|
| **할 일** | 할 일 추가·완료 토글·삭제, 필터(전체/진행중/완료), 실시간 개수 표시 |
| **스탑워치** | 시작/일시정지, 랩 기록, 랩별 구간 시간·삭제, 초기화. 실행 상태까지 저장되며, 실행 중에 앱을 종료하면 그 시점에 일시정지됨 |
| **통계** | 기간 선택 후 일별 통계: 완료/생성 할 일 수, 집중 시간(스탑워치 세션 기록), 랩 수·평균 랩 시간. 날짜는 로컬 시간대 기준이며, 삭제한 할 일도 통계에 남음. Chart.js 막대·선 차트, CSV 내보내기 |
| **백업** | Tauri 환경에서만: 비밀번호로 암호화한 JSON 파일로 내보내기·가져오기(할 일 + 스탑워치 + 통계 기록). 같은 비밀번호로 다른 PC에서도 복원 가능 |
| **실시간 시계** | 상단 헤더에 현재 요일·시각 표시 |

- Tauri가 없을 때(예: 브라우저에서 `index.html` 직접 열기)에는 **localStorage**로 할 일·스탑워치 상태를 저장하며, 백업/통계 일부는 Tauri 전용입니다.

---

### 기술 스택

- **프론트엔드**: HTML5, CSS3, Vanilla JavaScript (ES Module), [Chart.js](https://www.chartjs.org/) (통계 차트, `src/vendor/`에 포함되어 오프라인에서도 동작)
- **데스크톱·백엔드**: [Tauri v1](https://tauri.app/) (Rust)
- **데이터 저장**: Rust 측에서 **AES-256-GCM** 암호화 후 앱 데이터 디렉터리에 저장. 암호화 키는 같은 디렉터리의 `key_fallback.b64`를 우선 사용하고(키체인 비밀번호 창 방지), 없으면 OS 키체인(keyring)에서 가져옴
- **백업 파일**: 사용자 비밀번호에서 **Argon2id**로 만든 키로 AES-256-GCM 암호화. 이전 형식(v1: 기기 키 + HMAC) 백업도 만든 기기에서는 가져올 수 있음

---

### 요구 사항

- [Node.js](https://nodejs.org/) (npm 사용)
- [Rust](https://www.rust-lang.org/) (stable)
- Tauri 개발 환경: [공식 가이드 – Prerequisites](https://tauri.app/v1/guides/getting-started/prerequisites) 참고 (Windows: WebView2, macOS/Linux: 웹키트 등)

---

### 설치 및 실행

```bash
# 저장소 클론 후 프로젝트 루트에서
cd todo_app

# npm 의존성 설치 (@tauri-apps/cli 등)
npm install

# 개발 모드 실행 (프론트 빌드 없이 src/ 로드, Hot reload)
npm run tauri dev
```

첫 실행 시 Rust 빌드로 인해 시간이 걸릴 수 있습니다.

---

### 프로젝트 구조

```
todo_app/
├── src/                      # 프론트엔드 (Tauri가 이 디렉터리를 로드)
│   ├── index.html            # 단일 페이지: 할 일, 필터, 스탑워치, 통계 패널
│   ├── main.js               # 할 일/스탑워치/통계/백업 UI·로직, Tauri invoke
│   ├── styles.css            # 스타일
│   ├── vendor/chart.umd.js   # Chart.js 4.4.0 (로컬 포함, CSP로 외부 스크립트 차단)
│   └── assets/               # 정적 에셋
├── src-tauri/                # Tauri (Rust) 앱
│   ├── Cargo.toml            # Rust 의존성 (tauri, serde, aes-gcm, argon2, keyring, chrono 등)
│   ├── tauri.conf.json       # 창 크기, identifier, 허용 API 등
│   ├── icons/
│   └── src/
│       ├── main.rs           # 진입점, 데이터 모델, 통계, Tauri commands:
│       │                     #   할 일: get_tasks, add_task, toggle_task, delete_task
│       │                     #   스탑워치: get_stopwatch_state, stopwatch_start/stop/lap/
│       │                     #             delete_lap/clear_laps/reset
│       │                     #   백업: export_data, backup_needs_password, import_data
│       │                     #   통계: get_range_stats, export_stats_csv
│       │                     #   기타: take_load_failure (시작 시 로드 실패 알림)
│       ├── stopwatch.rs      # 스탑워치 상태와 집중 세션·랩 기록
│       └── storage.rs        # 암호화 저장/로드(원자적 쓰기), 키 관리, 백업 v1/v2
├── package.json              # scripts: tauri, tauri dev/build
└── README.md
```

---

### 데이터 저장 위치 (Tauri 앱)

- **앱 데이터**: OS별 앱 데이터 디렉터리 내 `app_data.enc.json` (암호화된 JSON).
- **암호화 키**: 같은 디렉터리의 `key_fallback.b64`. 없으면 OS 키체인(서비스명 = bundle identifier)에서 가져와 이 파일에 저장.
- **백업 파일**: 사용자가 지정한 경로에 저장되는 `.json` 파일(비밀번호 기반 암호화).
- **보존 파일**: 저장 데이터를 불러오지 못하면 원본을 `app_data.enc.json.corrupt-<시각>`으로, 교체된 키는 `key_fallback.b64.old-<시각>`으로 지우지 않고 남겨 둠.

#### 보호 범위

- 암호화 키가 데이터 파일과 같은 디렉터리에 있으므로, 로컬 암호화는 **같은 사용자 계정의 프로그램으로부터 데이터를 보호하지 않습니다.**
- 대신 macOS/Linux에서는 앱 데이터 디렉터리(0700)와 데이터·키 파일(0600)을 본인 계정만 접근할 수 있게 하여 **다른 사용자 계정으로부터** 보호합니다. Windows에서는 사용자별 앱 데이터 폴더 권한에 따릅니다.
- 백업 파일은 비밀번호로 암호화되므로 키 파일 없이 단독으로 보관·이동해도 됩니다. 비밀번호를 잊으면 복원할 수 없습니다.

---

### 빌드 (배포용 실행 파일)

```bash
npm run tauri build
```

실행 파일과 설치 패키지는 `src-tauri/target/release/` 및 Tauri가 생성하는 bundle 디렉터리에서 확인할 수 있습니다.

---

### 스크립트 요약

| 명령 | 설명 |
|------|------|
| `npm run tauri dev` | 개발 모드 실행 |
| `npm run tauri build` | 프로덕션 빌드 |

---

### 라이선스 / 기여

프로젝트에 별도 라이선스 파일이 없다면 저장소 소유자의 정책을 따릅니다. 기여는 이슈·풀 리퀘스트로 환영합니다.

---

<span id="ja"></span>

## 🇯🇵 日本語

# Todo App (Tauri + Vanilla)

Tauri と Vanilla JavaScript で作った **デスクトップ用 Todo・ストップウォッチ・統計アプリ**です。タスク管理、ストップウォッチ（ラップタイム）、期間別統計・グラフ、暗号化バックアップ/復元に対応しています。

---

### 主な機能

| 機能 | 説明 |
|------|------|
| **Todo** | タスクの追加・完了トグル・削除、フィルター（全体/進行中/完了）、件数表示 |
| **ストップウォッチ** | 開始/一時停止、ラップ記録、ラップごとの区間時間・削除、リセット。実行状態まで保存され、実行中にアプリを終了するとその時点で一時停止 |
| **統計** | 期間指定で日別統計：完了/作成タスク数、集中時間（ストップウォッチのセッション記録）、ラップ数・平均ラップ時間。日付はローカルタイムゾーン基準で、削除したタスクも統計に残る。Chart.js の棒・線グラフ、CSV エクスポート |
| **バックアップ** | Tauri 環境のみ：パスワードで暗号化した JSON でエクスポート・インポート（Todo + ストップウォッチ + 統計記録）。同じパスワードで別の PC でも復元可能 |
| **リアルタイム時計** | ヘッダーに現在の曜日・時刻を表示 |

- Tauri がない環境（例：ブラウザで `index.html` を直接開く場合）は **localStorage** で Todo・ストップウォッチ状態を保存します。バックアップや統計の一部は Tauri 専用です。

---

### 技術スタック

- **フロントエンド**: HTML5, CSS3, Vanilla JavaScript (ES Module), [Chart.js](https://www.chartjs.org/)（統計グラフ。`src/vendor/` に同梱しオフラインでも動作）
- **デスクトップ・バックエンド**: [Tauri v1](https://tauri.app/) (Rust)
- **データ保存**: Rust 側で **AES-256-GCM** 暗号化のうえアプリデータディレクトリに保存。暗号鍵は同じディレクトリの `key_fallback.b64` を優先し（キーチェーンのパスワード入力を回避）、無ければ OS キーチェーン(keyring)から取得
- **バックアップファイル**: ユーザーのパスワードから **Argon2id** で導出した鍵で AES-256-GCM 暗号化。旧形式（v1: 端末の鍵 + HMAC）のバックアップも作成した端末ではインポート可能

---

### 必要環境

- [Node.js](https://nodejs.org/) (npm)
- [Rust](https://www.rust-lang.org/) (stable)
- Tauri 開発環境: [公式ガイド – Prerequisites](https://tauri.app/v1/guides/getting-started/prerequisites) を参照（Windows: WebView2、macOS/Linux: WebKit 等）

---

### インストールと実行

```bash
# リポジトリ clone 後、プロジェクトルートで
cd todo_app

# npm 依存関係のインストール（@tauri-apps/cli 等）
npm install

# 開発モードで起動（src/ をそのまま読み込み、ホットリロード）
npm run tauri dev
```

初回実行時は Rust のビルドで時間がかかることがあります。

---

### プロジェクト構成

```
todo_app/
├── src/                      # フロントエンド（Tauri がこのディレクトリを読み込む）
│   ├── index.html            # 単一ページ: Todo、フィルター、ストップウォッチ、統計パネル
│   ├── main.js               # Todo/ストップウォッチ/統計/バックアップの UI・ロジック、Tauri invoke
│   ├── styles.css            # スタイル
│   ├── vendor/chart.umd.js   # Chart.js 4.4.0（同梱。CSP で外部スクリプトを遮断）
│   └── assets/               # 静的アセット
├── src-tauri/                # Tauri (Rust) アプリ
│   ├── Cargo.toml            # Rust 依存関係（tauri, serde, aes-gcm, argon2, keyring, chrono 等）
│   ├── tauri.conf.json       # ウィンドウサイズ、identifier、許可 API 等
│   ├── icons/
│   └── src/
│       ├── main.rs           # エントリポイント、データモデル、統計、Tauri commands:
│       │                     #   Todo: get_tasks, add_task, toggle_task, delete_task
│       │                     #   ストップウォッチ: get_stopwatch_state, stopwatch_start/stop/lap/
│       │                     #                     delete_lap/clear_laps/reset
│       │                     #   バックアップ: export_data, backup_needs_password, import_data
│       │                     #   統計: get_range_stats, export_stats_csv
│       │                     #   その他: take_load_failure（起動時の読み込み失敗通知）
│       ├── stopwatch.rs      # ストップウォッチの状態と集中セッション・ラップ記録
│       └── storage.rs        # 暗号化保存/読み込み（アトミック書き込み）、鍵管理、バックアップ v1/v2
├── package.json              # scripts: tauri, tauri dev/build
└── README.md
```

---

### データの保存場所（Tauri アプリ）

- **アプリデータ**: OS ごとのアプリデータディレクトリ内の `app_data.enc.json`（暗号化 JSON）。
- **暗号鍵**: 同一ディレクトリの `key_fallback.b64`。無ければ OS キーチェーン（サービス名 = bundle identifier）から取得してこのファイルに保存。
- **バックアップファイル**: ユーザーが指定したパスに保存される `.json`（パスワードベースの暗号化）。
- **保全ファイル**: 保存データを読み込めない場合は元ファイルを `app_data.enc.json.corrupt-<日時>` として、置き換えた鍵は `key_fallback.b64.old-<日時>` として削除せずに残す。

#### 保護範囲

- 暗号鍵がデータファイルと同じディレクトリにあるため、ローカル暗号化は**同じユーザーアカウントのプログラムからはデータを保護しません。**
- その代わり macOS/Linux ではアプリデータディレクトリ（0700）とデータ・鍵ファイル（0600）を本人のアカウントのみアクセス可能にし、**他のユーザーアカウントから**保護します。Windows ではユーザーごとのアプリデータフォルダの権限に従います。
- バックアップファイルはパスワードで暗号化されるため、鍵ファイルなしで単独で保管・移動できます。パスワードを忘れると復元できません。

---

### ビルド（配布用実行ファイル）

```bash
npm run tauri build
```

実行ファイルやインストーラーは `src-tauri/target/release/` および Tauri が生成する bundle ディレクトリで確認できます。

---

### スクリプト一覧

| コマンド | 説明 |
|----------|------|
| `npm run tauri dev` | 開発モードで起動 |
| `npm run tauri build` | 本番ビルド |

---

### ライセンス・コントリビューション

プロジェクトにライセンスファイルが無い場合はリポジトリ所有者のポリシーに従います。Issue・Pull Request での貢献を歓迎します。

---

<p align="center">
  <a href="#ko">한국어</a> · <a href="#ja">日本語</a>
</p>
