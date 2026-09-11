# 設計: bath-monitor — 「誰がお風呂に入ったか」を記録する家庭用IoTシステム

## 背景・目的
`bath-monitor`は、浴室の壁に取り付けるパネルで、5個の押しボタン(家族一人につき1個)と1個の在室用照度センサーを備え、Raspberry Pi Pico 2 Wに配線されています。ボタンを押すと「この人物は今日入浴した」という記録が残ります(1日単位で冪等、ローカル時刻の深夜0時にリセット) — 目的はあくまで今日誰が押した/押していないかを把握することであり、「最後に入ったのは誰か」を計算することではありません。照度センサーは独立して浴室が現在使用中かどうかを報告し、その入退室の履歴は恒久的にログとして保持されます。Pico 2 Wはどちらの信号も、MQTTではなく単純なHTTP POSTで自宅LAN上のFlaskサーバーに送信し、サーバーは自動更新される単一のHTMLダッシュボードを描画します。

**基板の変更履歴**: 当初の実装(以下参照)は元のPico W(RP2040)を対象としていましたが、ファームウェアを実機に一度も書き込む前の2026-09-03に、対象基板をPico 2 W(RP2350)へ変更する方針となり、同日中にファームウェアのコードもRP2350向けに移行しました(`thumbv8m.main-none-eabihf`向けにビルドとclippyの両方を通過済み、実機での書き込み確認はまだ未実施)。Pico 2 Wはピン互換(GPIO番号が同じ)のため、配線・ピンの計画には影響がありませんでした — マイグレーションの詳細な差分(ターゲットのtriple、`embassy-rp`のfeatureフラグ、ブートイメージ形式、書き込みツール、クロック分周比)については`CLAUDE.md`の「Notes for future work」を参照してください。

初期(Pico W)実装時に開発機(macOS)で使用したツールチェーン: rustc/cargo 1.93.0、`thumbv6m-none-eabi`ターゲットをインストール済み、`elf2uf2-rs`/`flip-link`/`cargo-generate`をPATHに追加(`probe-rs`は未使用 — 書き込みはSWDデバッグではなくBOOTSEL + UF2のドラッグ&ドロップで行う)。Python 3.14.6/pip 26.1.2。RP2350/Pico 2 Wへの移行後: `thumbv8m.main-none-eabihf`ターゲット、書き込みには`elf2uf2-rs`の代わりに`picotool`を使用(`elf2uf2-rs`はRP235x向けの動作するUF2を生成できないため)、リンカとしての`flip-link`はこのターゲットでも引き続き動作することを確認済み。

## リポジトリ構成
1つのリポジトリに2つの独立したプロジェクトが存在します(Cargoワークスペースではなく、Rustのクレートは1つだけ):
```
bath-monitor/
├── CLAUDE.md, README.md, .gitignore
├── docs/design.md           # このファイル
├── docs/protocol.md         # 正式なHTTP APIの契約(両側の信頼できる情報源)
├── docs/wiring.md           # GPIOピンの割り当て
├── firmware/                 # Rust、embassy-rp、独立したクレート
└── server/                   # Python、Flask
```

## ファームウェア(`firmware/`、Rust + embassy-rp + cyw43)

**主な設計判断:**
- ロギング: `defmt`/RTTではなく`embassy-usb-logger`(USB-CDC)+ `log` — デバッグプローブが手元にないため。ただし書き込みに使うのと同じケーブル経由でUSBシリアルが使える。
- パニックハンドラ: `panic-halt`(RTTベースの`panic-probe`はプローブなしでは意味がないため)。
- 設定: `src/secrets.rs`(gitignore対象、実際のWi-Fi/サーバー情報)+ `src/secrets.rs.example`(コミットされているテンプレート)+ `src/config.rs`(非秘匿情報: 人物リスト、GPIOピン、デバウンス時間など)。
- HTTPペイロード: `heapless::String` + `core::write!`による手作りの小さなJSON(`serde`は使わない — ボディはせいぜい1〜2フィールドのため)。
- タスク間の連携: `embassy_sync::channel::Channel<AppEvent, 8>`を1つ用意 — ボタン/在室タスクはイベントをプッシュするだけで、単一の`sender_task`が`reqwless::HttpClient`を所有し、チャンネルを直列に処理する(ロック不要で、自然にPOSTがレート制限される)。

**依存クレートのバージョン**(2026-08-30時点でcrates.ioの実際のリリースに対して検証済み — `net.rs`に依存する前に、これらの正確な固定バージョンで、`embassy-net`のスタック構築APIと`cyw43`のファームウェアBLOB読み込みを、実際に稼働している`embassy-rs/embassy`の`examples/rp/src/bin/wifi_*.rs`と突き合わせて再確認すること。これらのAPIはバージョンによって形が変わっているため):
```
embassy-executor 0.10.0, embassy-time 0.5.1, embassy-rp 0.10.0,
embassy-net 0.9.1 (tcp, dns, dhcpv4), embassy-sync 0.8.0, embassy-usb-logger 0.6.0,
cyw43 0.7.0, cyw43-pio 0.10.0,
reqwless 0.14.0, embedded-io-async 0.7.0,
cortex-m 0.7.9, cortex-m-rt 0.7.6, panic-halt 1.0.0,
static_cell 2.1.1, heapless 0.9.3, log 0.4.34
```
`rand_core`のバージョンが、実際に`embassy-rp 0.10.0`が依存しているものと一致しているかも確認すること。

**モジュール構成:**
- `main.rs` — エグゼキュータのセットアップ、ハードウェア初期化、全タスクのスポーン
- `secrets.rs` / `secrets.rs.example` / `config.rs` — 上記の通り
- `events.rs` — `AppEvent { ButtonPressed{person_idx}, OccupancyChanged{occupied} }`と、共有の`Channel`
- `net.rs` — cyw43/embassy-netの初期化、指数バックオフ付きリトライによるWi-Fi接続(一時的なAPの不調から自力で復旧できる必要がある — 動かなくなった基板をデバッグするプローブがないため)
- `buttons.rs` — `Input::wait_for_rising_edge()`を使った5個の非同期タスク、ピンごとに50msのデバウンス、`ButtonPressed`をプッシュ
- `occupancy.rs` — 照度センサーの読み取り(要件通りデジタル`Input`による2値信号)、しきい値付近のチャタリングを排除するための確定待ちデバウンス(約2000ms)を経てから`OccupancyChanged`をプッシュ
- `http_client.rs` — 単一の`reqwless::HttpClient`を所有し、チャンネルを処理して`/api/press`に`{"person":"alice"}`を、`/api/occupancy`に`{"occupied":true}`をPOSTし、`log::info!`/`warn!`で結果をログ出力

**ビルド/書き込み(移行後のRP2350/Pico 2 W):** `.cargo/config.toml`で`target = "thumbv8m.main-none-eabihf"`、`runner = "picotool load -u -v -x -t elf"`を設定(`elf2uf2-rs`のUF2出力はRP235xでは動作しないことをembassy-rs/embassy#4322で確認済み)。リンカには`flip-link`を使用(スタックオーバーフローのガードページ — デバッガがない環境では有用。このターゲットでも問題なくリンクできることを確認済み)。`memory.x`はRP2350のブートレイアウト(RP2040のBOOT2領域の代わりに`.start_block`/`.bi_entries`/`.end_block`セクション)を使用しています — `embassy-rp`の`rp235xa`featureが必要な`IMAGE_DEF`ブートブロックを自動的に生成してくれるため、`main.rs`側の変更は不要でした。`cargo run --release`でビルド → BOOTSELモードのPico 2 Wに`picotool`経由で書き込みます。

## サーバー(`server/`、Python + Flask)

**構成:** `Flask==3.1.3`(Jinja2を同梱)に加え、本番用WSGIサーバーとして`waitress==3.0.2`を使用 — このアプリはRaspberry Pi 4上でsystemd経由で24時間365日稼働するため(`docs/deploy.md`参照)、実際にデプロイされるのは`wsgi.py`/waitressの組み合わせです。`app.py`のFlask開発用サーバー(`python app.py`)はローカル開発用としてのみ残しています。永続化は標準ライブラリの`sqlite3`を使用(スキーマは小さなテーブル2つのみで、ORMは不要)。

**スキーマ(`schema.sql`):**
```sql
CREATE TABLE press_status (
    person TEXT PRIMARY KEY,
    last_pressed_at TEXT,        -- ISO8601、一度も押されていなければNULL
    last_pressed_date TEXT       -- 'YYYY-MM-DD'形式、リセット処理の判定に使用
);
CREATE TABLE occupancy_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occupied INTEGER NOT NULL CHECK (occupied IN (0,1)),
    changed_at TEXT NOT NULL     -- ISO8601、追記専用で、削除されることはない
);
```
`press_status`は起動時に`config.PEOPLE`から(`INSERT OR IGNORE`で)初期投入されます — 家族の追加は設定変更+再起動で対応します。現在の在室状態は、専用の「現在の状態」テーブルを持たず、`occupancy_log`の最新行そのものとします(こうすることで状態のズレを防いでいます)。

**日次リセット(スケジューラなし):** 読み書きのたびに`last_pressed_date`を`date.today()`(サーバーのローカル時刻。UTCのクラウドインフラではなく自宅LAN上で動いているため、標準ライブラリの`datetime`のみで`pytz`/`zoneinfo`は不要)と比較します。`pressed_today = (last_pressed_date == today)`。押下時の書き込みは無条件のupsertであり、同日中の再押下は害のない冪等な上書きになります。

**ルーティング(`app.py`、アプリファクトリパターン):**
| メソッド | パス | 用途 |
|---|---|---|
| POST | `/api/press` | `{"person": "<id>"}` → `config.PEOPLE`に対して検証(未知なら400)、`press_status`をupsert |
| POST | `/api/occupancy` | `{"occupied": true\|false}` → `occupancy_log`に追記(サーバー側での重複排除はせず、ファームウェア側のデバウンスを信頼する) |
| GET | `/api/status` | JSONスナップショット: 人物ごとの`pressed_today`/`last_pressed_at`、現在の`occupied`/`occupied_since` |
| GET | `/` | `/api/status`と同じ`db.get_status()`のデータを使って`templates/status.html`を描画 |

`db.py`は`init_db()`、`get_status()`、`record_press()`、`record_occupancy()`を公開しており、ルート側は薄いまま(パース → 呼び出し → 返却/描画)にしています。

**ページ:** `templates/base.html` + `status.html`、`<meta http-equiv="refresh" content="5">`(自宅LAN向けのダッシュボードなのでJS/WebSocketは不要) — 5人分を色付きのピル(押した/押していない)で表示し、在室状態のバナーを1つ表示(在室中/不在 + 開始時刻)。`static/style.css`で最小限のスタイリングを行います。

## 作成されたファイル
```
firmware/Cargo.toml, Cargo.lock, .cargo/config.toml, memory.x, build.rs
firmware/src/{main,secrets.rs.example,config,events,net,buttons,occupancy,http_client}.rs
server/{requirements.txt,config.py,schema.sql,db.py,models.py,app.py}
server/templates/{base,status}.html, server/static/style.css
server/tests/{conftest.py,test_api.py}
server/data/.gitkeep   (bath_monitor.dbは実行時状態のためgitignore対象)
docs/design.md, docs/protocol.md, docs/wiring.md
README.md, .gitignore  (target/, secrets.rs, __pycache__/, .venv/, *.db, .DS_Store)
```

## 検証手順
**サーバー(ハードウェア不要):**
```bash
cd server && python -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt
python app.py
# 別のターミナルで:
curl -X POST localhost:8080/api/press -H 'Content-Type: application/json' -d '{"person":"alice"}'
curl -X POST localhost:8080/api/occupancy -H 'Content-Type: application/json' -d '{"occupied":true}'
curl localhost:8080/api/status
open localhost:8080/
```
併せて確認すること: 未知の人物 → 400、不正なリクエストボディ → 400、sqliteファイル内の`last_pressed_date`を手動で過去日付にしてリセット処理を確認し、`/api/status`が`false`に切り替わることを確認する。`pytest server/tests/`を実行する。

**ファームウェア(書き込み後):** PicoをBOOTSELモードにして`cargo run --release` → USBシリアル端末を開く(`screen /dev/tty.usbmodemXXXX 115200`) → Wi-Fi接続とIPのログを確認し、その後、物理的なボタン押下によってサーバー側で`POST /api/press 200`が発生すること(Flaskのリクエストログで確認可能)、ダッシュボードのピルが1回の更新サイクル以内に緑色に切り替わること、そしてMAX98357Aアンプがビープ音(`audio.rs`)を、耳で聞いて分かるようなクリッピング/歪みなしに再生することを確認する — クリッピングする場合は`audio.rs`のウェーブテーブルの振幅を調整すること。照度センサーを覆う/覆いを外すを行い、実際の状態遷移1回につき、チャタリングによる大量発生ではなく、デバウンス済みの`occupancy_log`の行がちょうど1行だけ記録されることを確認する。
