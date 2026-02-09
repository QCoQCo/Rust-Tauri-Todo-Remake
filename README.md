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

Tauri와 Vanilla JavaScript로 만든 데스크톱 투두 앱입니다.

### 기술 스택

- **프론트엔드**: HTML, CSS, Vanilla JavaScript
- **백엔드/데스크톱**: [Tauri](https://tauri.app/) (Rust)

### 요구 사항

- [Node.js](https://nodejs.org/) (npm)
- [Rust](https://www.rust-lang.org/)
- Tauri 개발 환경 설정 ([공식 가이드](https://tauri.app/v1/guides/getting-started/prerequisites))

### 설치 및 실행

```bash
# 의존성 설치
npm install

# 개발 모드 실행
npm run tauri dev
```

### 프로젝트 구조

```
todo_app/
├── src/                 # 프론트엔드 (HTML, CSS, JS)
│   ├── index.html
│   ├── main.js
│   └── styles.css
└── src-tauri/           # Tauri (Rust)
    └── src/
        ├── main.rs      # 앱 진입점, IPC
        └── storage.rs   # 로컬 저장
```

### 빌드

```bash
npm run tauri build
```

---

<span id="ja"></span>

## 🇯🇵 日本語

# Todo App (Tauri + Vanilla)

Tauri と Vanilla JavaScript で作ったデスクトップ Todo アプリです。

### 技術スタック

- **フロントエンド**: HTML, CSS, Vanilla JavaScript
- **バックエンド/デスクトップ**: [Tauri](https://tauri.app/) (Rust)

### 必要環境

- [Node.js](https://nodejs.org/) (npm)
- [Rust](https://www.rust-lang.org/)
- Tauri 開発環境のセットアップ ([公式ガイド](https://tauri.app/v1/guides/getting-started/prerequisites))

### インストールと実行

```bash
# 依存関係のインストール
npm install

# 開発モードで起動
npm run tauri dev
```

### プロジェクト構成

```
todo_app/
├── src/                 # フロントエンド (HTML, CSS, JS)
│   ├── index.html
│   ├── main.js
│   └── styles.css
└── src-tauri/           # Tauri (Rust)
    └── src/
        ├── main.rs      # アプリのエントリポイント, IPC
        └── storage.rs   # ローカルストレージ
```

### ビルド

```bash
npm run tauri build
```

---

<p align="center">
  <a href="#ko">한국어</a> · <a href="#ja">日本語</a>
</p>
