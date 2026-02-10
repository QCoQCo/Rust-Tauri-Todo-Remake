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

<p align="center">
  <img src="demo/스크린샷%202026-02-10%20오전%2010.20.30.png" alt="Todo App 메인 화면" width="700">
</p>
<p align="center"><em>메인 화면 스크린샷</em></p>

---

### 주요 기능

| 기능 | 설명 |
|------|------|
| **할 일** | 할 일 추가·완료 토글·삭제, 필터(전체/진행중/완료), 실시간 개수 표시 |
| **스탑워치** | 시작/일시정지, 랩 기록, 랩별 구간 시간·삭제, 초기화. 상태는 앱과 함께 저장됨 |
| **통계** | 기간 선택 후 일별·주간 통계: 완료/생성 할 일 수, 포커스 시간, 랩 수. Chart.js 막대·선 차트, CSV 내보내기 |
| **백업** | Tauri 환경에서만: 암호화된 JSON 파일로 내보내기·가져오기(할 일 + 스탑워치 상태) |
| **실시간 시계** | 상단 헤더에 현재 요일·시각 표시 |

- Tauri가 없을 때(예: 브라우저에서 `index.html` 직접 열기)에는 **localStorage**로 할 일·스탑워치 상태를 저장하며, 백업/통계 일부는 Tauri 전용입니다.

---

### 기술 스택

- **프론트엔드**: HTML5, CSS3, Vanilla JavaScript (ES Module), [Chart.js](https://www.chartjs.org/) (통계 차트)
- **데스크톱·백엔드**: [Tauri v1](https://tauri.app/) (Rust)
- **데이터 저장**: Rust 측에서 **AES-256-GCM** 암호화 후 앱 데이터 디렉터리에 저장. 암호화 키는 OS 키체인(keyring) 우선 사용, 실패 시 로컬 fallback 파일 사용
- **백업 파일**: 내보내기 시 동일 키로 암호화 + HMAC 서명으로 변조 검증

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
│   └── assets/               # 정적 에셋
├── src-tauri/                # Tauri (Rust) 앱
│   ├── Cargo.toml            # Rust 의존성 (tauri, serde, aes-gcm, keyring, chrono 등)
│   ├── tauri.conf.json       # 창 크기, identifier, 허용 API 등
│   ├── icons/
│   └── src/
│       ├── main.rs           # 진입점, Tauri commands: get_tasks, add_task, toggle_task,
│       │                     # delete_task, get/set/clear_stopwatch_state, export_data,
│       │                     # import_data, get_daily_stats, get_weekly_stats, export_stats_csv
│       └── storage.rs        # AES-256-GCM 암호화 저장/로드, 키체인·fallback 키, 백업 내보내기/가져오기
├── package.json              # scripts: tauri, tauri dev/build
└── README.md
```

---

### 데이터 저장 위치 (Tauri 앱)

- **앱 데이터**: OS별 앱 데이터 디렉터리 내 `app_data.enc.json` (암호화된 JSON).
- **암호화 키**: OS 키체인(서비스명 = bundle identifier) 또는 동일 디렉터리의 `key_fallback.b64`.
- **백업 파일**: 사용자가 지정한 경로에 저장되는 `.json` 파일(동일 형식·HMAC 서명 포함).

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

<p align="center">
  <img src="demo/스크린샷%202026-02-10%20오전%2010.20.30.png" alt="Todo App メイン画面" width="700">
</p>
<p align="center"><em>メイン画面スクリーンショット</em></p>

---

### 主な機能

| 機能 | 説明 |
|------|------|
| **Todo** | タスクの追加・完了トグル・削除、フィルター（全体/進行中/完了）、件数表示 |
| **ストップウォッチ** | 開始/一時停止、ラップ記録、ラップごとの区間時間・削除、リセット。状態はアプリとともに保存 |
| **統計** | 期間指定で日別・週間統計：完了/作成タスク数、フォーカス時間、ラップ数。Chart.js の棒・線グラフ、CSV エクスポート |
| **バックアップ** | Tauri 環境のみ：暗号化 JSON でエクスポート・インポート（Todo + ストップウォッチ状態） |
| **リアルタイム時計** | ヘッダーに現在の曜日・時刻を表示 |

- Tauri がない環境（例：ブラウザで `index.html` を直接開く場合）は **localStorage** で Todo・ストップウォッチ状態を保存します。バックアップや統計の一部は Tauri 専用です。

---

### 技術スタック

- **フロントエンド**: HTML5, CSS3, Vanilla JavaScript (ES Module), [Chart.js](https://www.chartjs.org/)（統計グラフ）
- **デスクトップ・バックエンド**: [Tauri v1](https://tauri.app/) (Rust)
- **データ保存**: Rust 側で **AES-256-GCM** 暗号化のうえアプリデータディレクトリに保存。暗号鍵は OS キーチェーン(keyring)を優先、失敗時はローカル fallback ファイルを使用
- **バックアップファイル**: エクスポート時は同一鍵で暗号化し、HMAC 署名で改ざん検証

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
│   └── assets/               # 静的アセット
├── src-tauri/                # Tauri (Rust) アプリ
│   ├── Cargo.toml            # Rust 依存関係（tauri, serde, aes-gcm, keyring, chrono 等）
│   ├── tauri.conf.json       # ウィンドウサイズ、identifier、許可 API 等
│   ├── icons/
│   └── src/
│       ├── main.rs           # エントリポイント、Tauri commands: get_tasks, add_task, toggle_task,
│       │                     # delete_task, get/set/clear_stopwatch_state, export_data,
│       │                     # import_data, get_daily_stats, get_weekly_stats, export_stats_csv
│       └── storage.rs        # AES-256-GCM 暗号化の保存/読み込み、キーチェーン・fallback 鍵、バックアップ出力/読み込み
├── package.json              # scripts: tauri, tauri dev/build
└── README.md
```

---

### データの保存場所（Tauri アプリ）

- **アプリデータ**: OS ごとのアプリデータディレクトリ内の `app_data.enc.json`（暗号化 JSON）。
- **暗号鍵**: OS キーチェーン（サービス名 = bundle identifier）または同一ディレクトリの `key_fallback.b64`。
- **バックアップファイル**: ユーザーが指定したパスに保存される `.json`（同一形式・HMAC 署名付き）。

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
