# rview

[![CI](https://github.com/ruribou/rview/actions/workflows/ci.yml/badge.svg)](https://github.com/ruribou/rview/actions/workflows/ci.yml)

ブランチ間の差分をDB / API / Logic / Test / Config に分けて表示するCLI。
大きめのPRで「どこを見ればいいか」をざっと掴むために作っている。Rustの勉強も兼ねている。

## インストール

```bash
cargo install --git https://github.com/ruribou/rview
```

## 使い方

```bash
rview                    # main...HEAD
rview develop            # develop...HEAD
rview main --head feat   # main...feat
rview -C path/to/repo    # 別ディレクトリのリポジトリ
rview --format json      # JSON出力
```

```text
Changes: main...HEAD

[DB]
  A db/migrate/20260926_add_status_to_users.rb (+6 -0)
  M db/schema.rb (+2 -2)

[API]
  M config/routes.rb (+1 -1)

[Logic]
  M app/services/user_service.rb (+3 -1)

[Config]
  M Gemfile (+2 -2)

⚠ Possible concerns
  - Destructive migration: db/migrate/20260926_add_status_to_users.rb
      remove_column :users, :legacy_flag, :string
  - Dependencies changed (Gemfile)
      ~ rails 7.1.0 → 7.2.0
      + sidekiq ~> 7.0
  - Code changed without test changes
```

分類はパスで決めている (Test → DB → API → Logic → Config の順で最初にマッチしたもの)。
concerns ではマイグレーションの破壊的変更、`schema.rb` のテーブル・カラム増減、`routes.rb` の変更、依存の増減、本番設定の変更、ファイル削除、テストなしのコード変更あたりを拾う。
細かいルールは [docs/design.md](docs/design.md) に書いてある。

## この先やりたいこと

- [x] 差分の分類
- [x] diff本文の解析 (migration / routes / 依存など)
- [ ] Controller → Service → Model → spec の関連ファイル探索
- [ ] `rview pr 123` でGitHubのPRを解析
- [ ] 必要な差分だけ絞ってLLMに渡す

## 開発

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT
