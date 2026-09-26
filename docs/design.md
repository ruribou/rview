# rview 設計ドキュメント (Phase 1 / MVP)

## ゴール

`rview <BASE>` で `BASE...HEAD` の変更ファイルを取得し、レビュー観点のカテゴリ
(DB / API / Logic / Test / Config / Other) に分類して表示する。
加えて、ファイルパスだけから判断できる「Possible concerns」を提示する。

## パイプライン

```text
git diff --name-status -z -M BASE...HEAD
        │  (git.rs)       生の出力 (String)
        ▼
parse_name_status          (diff.rs)
        │                  Vec<FileChange>
        ▼
Report::build              (report.rs)
   ├─ classify(path)       (category.rs)  → Category
   └─ concern::detect      (concern.rs)   → Vec<Concern>
        │
        ▼
render_text / render_json  (report.rs)
```

各段は純粋関数に近い形で分離しており、`git.rs` 以外は外部プロセスに依存しない。
そのためユニットテストは文字列入力だけで完結する。

## モジュール

| モジュール | 責務 | 主な型 |
| --- | --- | --- |
| `git` | `git` コマンドの実行とエラー整形 | `GitError` |
| `diff` | `--name-status -z` 出力のパース | `ChangeKind`, `FileChange`, `ParseError` |
| `category` | パスからカテゴリへの分類 | `Category`, `classify()` |
| `concern` | レビュー時に注意すべきシグナルの検出 | `Concern`, `detect()` |
| `report` | 集約・ソート・出力 | `Report`, `Group` |
| `main` | CLI 引数 (clap) とパイプラインの接続 | `Cli`, `Format` |

## 設計判断

### `-z` オプションでパースする

`git diff --name-status` の通常出力は、特殊文字を含むパスをクォート・エスケープする。
`-z` を使うとフィールドが NUL 区切りになり、パスが加工されないため、
タブや空白、非 ASCII 文字を含むパスでも確実にパースできる。

### 三点リーダ (`BASE...HEAD`)

Pull Request と同じく「`HEAD` と `BASE` のマージベース」からの差分を見る。
`BASE` 側だけで進んだ変更はレビュー対象に含めない。

### リネーム検出 (`-M`)

リネームを `D` + `A` ではなく `R` として扱い、旧パスも保持する
(`FileChange::old_path`)。分類は新パスで行う。

### 分類ルールの優先順位

最初にマッチしたルールを採用する。より具体的なルールを先に評価する。

1. **Test** — `spec/`, `test/`, `tests/`, `__tests__/` ディレクトリ、`*_spec.rb`, `*.test.ts` などの命名
2. **DB** — `db/migrate/`, `db/schema.rb`, `migrations/`, `*.sql` など
3. **API** — `app/controllers/`, `config/routes.rb`, OpenAPI 定義など
4. **Logic** — `app/models/`, `app/services/`, `lib/`, `src/` など
5. **Config** — `config/`, `.github/`, 依存マニフェスト、ルート直下の設定ファイルなど
6. **Other** — 上記以外

例えば `spec/models/user_spec.rb` は Logic ではなく Test、
`config/routes.rb` は Config ではなく API になる。
ディレクトリ判定はパスの構成要素単位で行い、`contest.rb` のような部分一致は避ける。

### Possible concerns

MVP ではパスと変更種別のみから判定する。

| Concern | 条件 |
| --- | --- |
| Migration detected | `db/migrate/` や `migrations/` のファイルが追加・変更された |
| DB schema changed | `db/schema.rb` などのスキーマダンプが変更された |
| API route changed | `config/routes.rb` / `config/routes/` が変更された |
| Dependencies changed | `Gemfile.lock`, `Cargo.toml` などが変更された |
| N file(s) deleted | 削除されたファイルがある |
| Code changed without test changes | Logic / API が変更されたが Test が 1 件も変更されていない |

### 出力形式

- `text` (デフォルト): 人間向け。カテゴリ順、各カテゴリ内はパス順にソート。
- `json`: 機械・LLM 向け。Phase 5 でこの構造化データを LLM への入力にする想定。

### エラー処理

- ライブラリ側は `thiserror` で型付きエラー (`GitError`, `ParseError`) を定義する。
- バイナリ側は `anyhow` でコンテキストを付けて表示し、非ゼロで終了する。

## テスト戦略

- **ユニットテスト**: 各モジュールに `#[cfg(test)]`。パース・分類・検出・描画を文字列だけで検証。
- **E2E テスト** (`tests/cli.rs`): 一時ディレクトリに実際の git リポジトリを作り、
  ブランチを切って変更をコミットし、バイナリの出力全体を検証する。

## CI

GitHub Actions (`.github/workflows/ci.yml`)

- `lint`: `cargo fmt --check`, `cargo clippy -D warnings`
- `test`: Linux / macOS / Windows で `cargo test`
- `dogfood`: PR 上で rview 自身を実行し、結果を Job Summary に出力

## 今後の拡張ポイント

- **Phase 2 (Diff Analyzer)**: `git.rs` に `git diff -U0` 取得を追加し、`FileChange` に hunk 情報を持たせる。
  `concern` を「パスベース」と「内容ベース」の検出器に分割する。
- **Phase 3 (Framework Awareness)**: `category.rs` のルールを `Framework` trait で差し替え可能にする
  (Rails, Next.js, Rust など)。Controller ↔ Service ↔ Model ↔ Spec の対応付けを追加。
- **Phase 4 (GitHub)**: `rview pr <N>` サブコマンド。`gh` もしくは GitHub API から変更一覧を取得し、
  `Vec<FileChange>` に変換すれば以降の処理は共通。
- **Phase 5 (AI)**: JSON 出力を元に、重要ファイルの diff だけを抽出して LLM に渡す。
