# WinNotiCatcher 🔔

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## ■ 概要 (Overview)
**WinNotiCatcher** は、Windows 10/11 に届いたデスクトップ通知（Discordなど）を自動的にキャプチャし、ログとしてCSVファイルに保存・閲覧できる軽量な常駐アプリです。

時間が経つとWindowsの通知センターから消えてしまう過去の通知も、このアプリがあれば後からいつでも確認・検索することができます。

---

## ■ 機能一覧 (Features)

*   ✅ **通知ログの自動保存・閲覧**：届いた通知を瞬時にキャプチャし、一覧表示とCSVファイルへの永続保存を行います。
*   🔍 **強力な検索とハイライト表示**：アプリ名やテキストの部分一致検索に対応し、ヒットしたキーワードだけを見やすくハイライトさせます。
*   📑 **アプリ別タブ管理**：リスト内の「アプリ名」をクリックするだけで、そのアプリだけの専用タブを瞬時に作成してフィルタリングできます。
*   🗃️ **自動アーカイブ設定**：古いログが溜まりすぎないよう、指定件数を超えた場合や日を跨いだ場合に自動的に「logs」フォルダ内の過去別ファイルへと移動させる設定（Settingsタブ）が可能です。
*   🔃 **ログのソート機能**：新着順・古い順の切り替えがワンクリックで可能です。

---

## ■ ダウンロードと使い方 (Usage)

1. [Releases](../../releases) ページから最新の `WinNotiCatcher_Release.zip` をダウンロードして解凍します。
2. フォルダ内の `WinNotiCatcher.exe` をダブルクリックして起動します。
3. そのまま開いておく（あるいはタスクバーに置いておく）だけで、Windowsに届いた通知が自動的にアプリ内のリストに追加され、同時に `logs/notifications_history.csv` ファイルに記録されます。

> **⚠ 初回起動時の注意（必須）**
> * 初回起動時にWindowsの**通知アクセスの許可**を求められるダイアログが表示された場合は、必ず**「許可」**に設定してください。
> * **SmartScreen（青い警告画面）について**：個人開発のフリーウェアであるため、初回起動時にWindowsのSmart App Controlなどによって「Windows によって PC が保護されました」という真っ青な警告画面が出ることがあります。その場合は、画面内の **「詳細情報」** をクリックし、右下の **「実行」** ボタンを押すことでお使いいただけます。ソースコードは本リポジトリで全て公開しており安全です。

---

## ■ 制限事項 (Limitations)
*   画像などのメディア付き通知が来た場合、画像自体は保存されません。
*   一部のOS標準カラー絵文字がそのまま記録されると化けてしまうトラブルを防ぐため、Discord風のショートコード（例： `:fire:` や `:heart:` など）に自動変換されてテキストとして保存されます。

---

## ■ アンインストール (Uninstall)
レジストリなどは一切使用していません。  
本ツールが不要になった場合は、ダウンロード・作成された `WinNotiCatcher.exe` と `logs` フォルダをそのまま「ゴミ箱」へ削除するだけで完全に消去できます。

---

## ■ 開発・ビルド方法 (For Developers)

このアプリはRustと `egui` (GUIフレームワーク) を使用して書かれています。

### 必要な環境
*   Rust (Cargo)
*   Windows 10 または 11 SDK

### クローンとビルド
```powershell
git clone https://github.com/USERNAME/WinNotiCatcher.git
cd WinNotiCatcher

# リリースビルドの作成（最適化＆コンソール非表示）
cargo build --release
```
ビルドされた実行ファイルは `target/release/WinNotiCatcher.exe` に生成されます。
