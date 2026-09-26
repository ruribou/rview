# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## コマンド

CI (`.github/workflows/ci.yml`) と同じチェック:

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- 単一テスト: `cargo test <名前の一部>` (例: `cargo test reports_renames`)
- E2E だけ: `cargo test --test cli`
- ライブラリのユニットテストだけ: `cargo test --lib`
- 手元で実行: `cargo run -- main` / `cargo run -- main --format json`

CI のテストは Linux / macOS / Windows で走るので、パス区切りや改行コードに依存するコードは避ける。
PR では `dogfood` ジョブが rview 自身をベースブランチに対して実行する。

## アーキテクチャ

`rview <BASE>` は `BASE...HEAD` (merge-base からの差分、PR と同じ) を解析する。処理は 3 段階:

1. `analysis::collect` — I/O はここと `git.rs` だけ。`git diff --name-status -z` (`diff`)、`git diff -U0` (`patch`)、
   merge-base と HEAD の両方から `git cat-file blob` で取った全文を比較 (`deps`: Gemfile / package.json / Cargo.toml、`schema`: `db/schema.rb`) し、
   純粋なデータの `Analysis` にまとめる。
2. `report::Report::build` — `category::classify` でパスをカテゴリに振り分け、`concern::detect` (+ `migration`) で Possible concerns を出す。
3. `render_text` / `render_json` で出力。

`Analysis` を境に I/O と判定・表示が分かれているので、`concern` や `report` のテストは git を使わず `Analysis::from_changes` でデータを組み立てて書く。
E2E (`tests/cli.rs`) は一時ディレクトリに実際の git リポジトリを作り、バイナリの出力全体を検証する。

押さえておくべき前提:

- カテゴリ判定は Test → DB → API → Logic → Config → Other の順で最初にマッチしたもの。ディレクトリ判定はパス要素単位 (`contest.rb` を test 扱いしない)。リネームは新パスで分類する。
- git の出力はユーザー設定に左右されないよう、`--no-color --no-ext-diff --no-textconv --no-relative`、`--src-prefix=a/ --dst-prefix=b/`、`core.quotePath=false` を明示している。git 呼び出しを足すときも同様にする。
- マニフェストやスキーマは diff 行ではなく全文をパースして比較する (`-U0` ではどのセクションの行か分からないため)。パースに失敗しても全体は止めず、`Analysis::warnings` 経由で stderr に警告を出す。
- エラーはライブラリ側が `thiserror` の型付きエラー、`main.rs` が `anyhow`。

設計の詳細・判断理由・今後のフェーズ (Framework Awareness、`rview pr <N>`、LLM 連携) は `docs/design.md` にある。
