# rview

[![CI](https://github.com/ruribou/rview/actions/workflows/ci.yml/badge.svg)](https://github.com/ruribou/rview/actions/workflows/ci.yml)

Gitの差分を解析し、コードレビューで確認すべき変更を整理するRust製CLIツール。

## 概要

`rview` は、Gitブランチ間の差分を取得・解析し、変更されたファイルを役割ごとに分類して表示するCLIツール。

単純に `git diff` の結果を表示するのではなく、

- DB
- API
- Business Logic
- Test
- Config
- Other

などのカテゴリに変更を整理することで、レビュー対象の全体像を素早く把握できるようにする。

将来的にはファイル間の関連性や変更内容まで解析し、レビュー時に重点的に確認すべきポイントを抽出する。

## インストール

```bash
cargo install --git https://github.com/ruribou/rview
```

ソースからビルドする場合:

```bash
git clone https://github.com/ruribou/rview
cd rview
cargo build --release
./target/release/rview --help
```

## 使い方

```bash
rview                    # main...HEAD を解析
rview develop            # develop...HEAD を解析
rview main --head feat   # main...feat を解析
rview -C path/to/repo    # 別ディレクトリのリポジトリを解析
rview --format json      # JSON で出力 (LLM やスクリプト向け)
```

### 出力例

```text
Changes: main...HEAD

[DB]
  A db/migrate/20260926_add_status_to_users.rb
  M db/schema.rb

[API]
  M app/controllers/api/users_controller.rb
  M config/routes.rb

[Logic]
  D app/services/old_service.rb
  M app/services/user_service.rb

[Test]
  M spec/services/user_service_spec.rb

[Config]
  M config/application.rb

⚠ Possible concerns
  - Migration detected
  - DB schema changed
  - API route changed
  - 1 file(s) deleted
```

各行の先頭は変更種別 (`A` Added / `M` Modified / `D` Deleted / `R` Renamed / `C` Copied / `T` TypeChanged)。
リネームは `R old/path -> new/path` と表示される。

### 分類ルール

| カテゴリ | 例 |
| --- | --- |
| DB | `db/migrate/**`, `db/schema.rb`, `db/structure.sql`, `**/migrations/**`, `*.sql` |
| API | `app/controllers/**`, `config/routes.rb`, `app/graphql/**`, `openapi.yaml` |
| Logic | `app/models/**`, `app/services/**`, `app/jobs/**`, `lib/**`, `src/**` |
| Test | `spec/**`, `test/**`, `tests/**`, `__tests__/**`, `*_spec.rb`, `*.test.ts` |
| Config | `config/**`, `.github/**`, `Gemfile`, `package.json`, `Cargo.toml`, `Dockerfile` |
| Other | 上記以外 |

Test → DB → API → Logic → Config の順に評価し、最初にマッチしたカテゴリになる。
(`spec/models/user_spec.rb` は Test、`config/routes.rb` は API)

### Possible concerns

| 表示 | 条件 |
| --- | --- |
| Migration detected | マイグレーションファイルの追加・変更 |
| DB schema changed | `db/schema.rb` などの変更 |
| API route changed | `config/routes.rb` の変更 |
| Dependencies changed | `Gemfile.lock`, `package.json`, `Cargo.toml` などの変更 |
| N file(s) deleted | ファイル削除 |
| Code changed without test changes | Logic / API が変わったのにテストの変更がない |

設計の詳細は [docs/design.md](docs/design.md) を参照。

---

## 背景

大きなPull Requestでは、多数のファイルが変更されるため、

- どこが主要な変更なのか
- DBやAPIに影響する変更があるか
- 実装に対応するテストが存在するか
- どのファイル同士が関連しているか

を把握するだけでもコストがかかる。

また、LLMにコードレビューを依頼する場合も、Pull Request全体のdiffをそのまま渡すと、不要なコンテキストが増加する。

そこで `rview` では、まずローカルでGit差分を構造化し、

> 「何が変更されたのか」

を機械的に整理する。

将来的には、

> 「レビューで何を見るべきなのか」

まで抽出し、必要な差分だけをLLMへ渡せるレビュー支援ツールを目指す。

## 技術スタック

- Rust
- Git (`git diff --name-status -z -M` を `std::process::Command` で実行)
- [`clap`](https://crates.io/crates/clap) — CLI引数解析
- [`thiserror`](https://crates.io/crates/thiserror) / [`anyhow`](https://crates.io/crates/anyhow) — エラー処理
- [`serde`](https://crates.io/crates/serde) / [`serde_json`](https://crates.io/crates/serde_json) — JSON出力

## Rust学習としての目的

単純なTodoアプリではなく、実際に利用できるCLIを作りながらRustの基本概念を学ぶ。

特に以下を扱う。

- Ownership / Borrowing
- `struct`
- `enum`
- `match`
- `Option`
- `Result`
- Error Handling
- Iterator
- Collection
- File / Process操作
- CLI設計

将来的には並列処理や非同期処理についても扱う。

## ロードマップ

### Phase 1 — Git Diff Organizer ✅

Gitの変更ファイルを取得して分類する。

```text
Git → diff取得 → parse → categorize → terminal output
```

### Phase 2 — Diff Analyzer

ファイル名だけでなくdiff本文を解析し、Migration追加・カラム削除・Routes変更・Dependency変更・Config変更などを検出する。

### Phase 3 — Framework Awareness

Railsなどのフレームワーク構造を理解し、Controller → Service → Model → Test といった関連ファイルを探索する。

```text
Controller changed
Service changed
Model changed
Test unchanged
```

### Phase 4 — GitHub Integration

```bash
rview pr 123
```

Pull Requestの変更を取得し、ローカルと同じ解析処理を適用する。

### Phase 5 — AI Review Assistant

Pull Request全体をそのままLLMへ送信するのではなく、

```text
Git Diff → Rustによる解析 → 変更のグルーピング → 重要ファイル抽出 → 関連diff抽出 → LLM
```

という構成で、LLMへ渡す情報量を減らしながらレビュー精度を高める。

## 最終的な方向性

単なるGit diff viewerではなく、

**「Pull Requestの変更構造を理解し、人間やAIがレビューしやすい形へ変換するツール」**

を目指す。

```text
PR #123

Main Changes
├── User status feature
│   ├── Migration
│   ├── Model
│   ├── Service
│   └── Controller
│
└── API route update
    ├── routes.rb
    └── users_controller.rb

Review Points
├── DB migration included
├── API contract changed
└── No corresponding test change detected
```

## 開発

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT
