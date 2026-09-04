# kicad_bus_gen

KiCad 10 のスケマティックファイル（`.kicad_sch`）にバス配線要素を自動生成するコマンドラインツールです。

[English README](README.md)

バスエントリ・ワイヤー・ネットラベルをピンの座標と方向から自動計算して追記します。KiCad の Python API（スケマティック編集）は未実装のため、ファイルを直接編集する方式を採用しています。

## 動作確認環境

- KiCad 10
- Rust 1.75以降
- macOS / Linux

## ビルド

```bash
git clone ...
cd kicad_bus_gen
cargo build --release
```

ビルド後のバイナリは `target/release/kicad_bus_gen` に生成されます。

## 使い方

```bash
./kicad_bus_gen <schematic.kicad_sch>
```

TUIが起動します。各行にバスを引き出したいピングループを入力し、`g` キーで生成するとスケマティックファイルに書き込まれます。

### TUI の操作

| キー | 動作 |
|------|------|
| `←` `→` | 列移動 |
| `↑` `↓` | 行移動 |
| `Tab` / `Shift+Tab` | 次列 / 前列 |
| `Enter` | セルを編集して確定、次列へ |
| `a` | 行を追加 |
| `d` | 行を削除 |
| `g` | バス要素を生成してファイルに書き込み |
| `q` / `Esc` | 生成せずに終了 |

### 入力カラム

| カラム | 説明 | 例 |
|--------|------|-----|
| Ref | シンボルのリファレンス | `U1` |
| Pin Prefix | ピン名のフォーマット（`%d` を含む） | `A_{%d}` `A%d` `D_%d` |
| Prefix | 生成するネットラベルのプレフィックス | `A` `D` |
| Start | 開始番号 | `0` |
| End | 終了番号（逆順も可） | `15` |
| Wire(inch) | ワイヤー長（インチ単位） | `0.2` |

### Pin Prefix のフォーマット

| フォーマット | マッチするピン名 |
|-------------|----------------|
| `A%d` | `A0`, `A1`, ... `A15` |
| `A_%d` | `A_0`, `A_1`, ... |
| `A_{%d}` | `A_{0}`, `A_{1}`, ... |

ピン名の候補は TUI の Suggestions 欄に表示されます。

### 生成例

以下の設定で Z80 のアドレスバス A0〜A15 のバス配線を生成できます。

```
Ref: U1    Pin Prefix: A_{%d}   Prefix: A   Start: 0   End: 15   Wire: 0.2
```

## デバッグコマンド

```bash
# ピン名とフォーマット一覧を表示
./kicad_bus_gen --list-pins U1 schematic.kicad_sch

# 特定ピンの座標・方向を表示
./kicad_bus_gen --pin-detail U1 "A_{0}" schematic.kicad_sch

# 全ピンの詳細を表示
./kicad_bus_gen --pin-detail U1 "*" schematic.kicad_sch
```

## 生成される要素

各ピンに対して以下の3要素を生成します。

```
(wire)       ピン接続点 → バスエントリ始点
(bus_entry)  バスエントリ（斜め線）
(label)      ネットラベル（バスエントリ端点に配置）
```

ピンの突き出し方向（Right / Left / Up / Down）はシンボルの回転・ミラー設定から自動判定します。

## セッション保存

生成時の入力内容は `<schematic>.bus_gen.json` に自動保存されます。次回起動時に前回の入力が復元されます。

## 注意事項

- 生成はファイルへの**追記**です。同じ設定で複数回実行すると要素が重複します。重複した場合は KiCad の Undo または手動削除で対処してください。
- ピンの座標変換は KiCad の TRANSFORM 行列仕様に基づいて実装しています。シンボルの回転（0°/90°/180°/270°）およびミラー（X軸・Y軸）に対応しています。

## ファイル構成

```
src/
├── main.rs       エントリポイント・デバッグコマンド
├── parser.rs     .kicad_sch S式パーサー・ピン座標計算・方向判定
├── generator.rs  バス要素生成・ファイル書き込み
├── tui.rs        ratatui TUI
└── session.rs    セッション保存（JSON）
```

## 依存クレート

```toml
uuid = { version = "1", features = ["v4"] }
rand = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ratatui = "0.29"
crossterm = "0.28"
```
