# 設計: bath-monitor — 「誰がお風呂に入ったか」を記録する家庭用IoTシステム

## 背景・目的
`bath-monitor`は、浴室の壁に取り付けるパネルで、5個の押しボタン(家族一人につき1個)と1個の在室用照度センサーを備え、Raspberry Pi Pico 2 Wに配線されています。あわせて、各ボタンに対応するNeoPixel(その人が今日押していれば点灯)と、ボタン押下時にその人専用のメロディを鳴らすI2Sアンプ(MAX98357A)+スピーカーも搭載します。ボタンを押すと「この人物は今日入浴した」という記録が残ります(1日単位で冪等、ローカル時刻の深夜0時にリセット。押し間違いなどは、ボタンの長押しで取り消せる) — 目的はあくまで今日誰が押した/押していないかを把握することであり、「最後に入ったのは誰か」を計算することではありません。照度センサーは独立して浴室が現在使用中かどうかを報告し、その入退室の履歴は恒久的にログとして保持されます。Pico 2 Wはどちらの信号も、MQTTではなく単純なHTTP POSTで自宅LAN上のFlaskサーバーに送信し、サーバーは自動更新される単一のHTMLダッシュボードを描画します。

**基板の変更履歴**: 当初の実装(以下参照)は元のPico W(RP2040)を対象としていましたが、ファームウェアを実機に一度も書き込む前の2026-09-03に、対象基板をPico 2 W(RP2350)へ変更する方針となり、同日中にファームウェアのコードもRP2350向けに移行しました(`thumbv8m.main-none-eabihf`向けにビルドとclippyの両方を通過済み。その後、2026-09-19に実機のPico 2 Wへ書き込み、Wi-Fi接続、ボタン、メロディ、オンボードLED、サーバーへの送信の動作を確認済み。未確認の項目は`CLAUDE.md`の「Notes for future work」を参照)。Pico 2 Wはピン互換(GPIO番号が同じ)のため、配線・ピンの計画には影響がありませんでした — マイグレーションの詳細な差分(ターゲットのtriple、`embassy-rp`のfeatureフラグ、ブートイメージ形式、書き込みツール、クロック分周比)については`CLAUDE.md`の「Notes for future work」を参照してください。

初期(Pico W)実装時に開発機(macOS)で使用したツールチェーン: rustc/cargo 1.93.0、`thumbv6m-none-eabi`ターゲットをインストール済み、`elf2uf2-rs`/`flip-link`/`cargo-generate`をPATHに追加(`probe-rs`は未使用 — 書き込みはSWDデバッグではなくBOOTSEL + UF2のドラッグ&ドロップで行う)。Python 3.14.6/pip 26.1.2。RP2350/Pico 2 Wへの移行後: `thumbv8m.main-none-eabihf`ターゲット、書き込みには`elf2uf2-rs`の代わりに`picotool`を使用(`elf2uf2-rs`はRP235x向けの動作するUF2を生成できないため)、リンカとしての`flip-link`はこのターゲットでも引き続き動作することを確認済み。

## リポジトリ構成
1つのリポジトリに2つの独立したプロジェクトが存在します(Cargoワークスペースではなく、Rustのクレートは1つだけ):
```
bath-monitor/
├── CLAUDE.md, README.md, .gitignore
├── docs/design.md           # このファイル(実装済みの内容)
├── docs/additional_spec.md  # 追加していく外部仕様(実装後も項目は残し、状態を更新する)
├── docs/protocol.md         # 正式なHTTP APIの契約(両側の信頼できる情報源)
├── docs/wiring.md           # GPIOピン・PIO/DMAの割り当て
├── docs/deploy.md           # Raspberry Pi 4へのサーバーのデプロイ手順(systemd)
├── firmware/                 # Rust、embassy-rp、独立したクレート
├── schematic/                # KiCadの回路図/基板データ
└── server/                   # Python、Flask
```

## ファームウェア(`firmware/`、Rust + embassy-rp + cyw43)

**主な設計判断:**
- ロギング: `defmt`/RTTではなく`embassy-usb-logger`(USB-CDC)+ `log` — デバッグプローブが手元にないため。ただし書き込みに使うのと同じケーブル経由でUSBシリアルが使える。
- パニックハンドラ: `panic-halt`(RTTベースの`panic-probe`はプローブなしでは意味がないため)。
- 設定: `src/secrets.rs`(gitignore対象、実際のWi-Fi/サーバー情報)+ `src/secrets.rs.example`(コミットされているテンプレート)+ `src/config.rs`(非秘匿情報: 人物リスト、GPIOピン、デバウンス時間など)。
- HTTPペイロード: `heapless::String` + `core::write!`による手作りの小さなJSON(`serde`は使わない — ボディはせいぜい1〜2フィールドのため)。
- タスク間の連携: 用途別の`embassy_sync::channel::Channel`を`events.rs`に用意 — NeoPixel用の`LED_CHANNEL`(CORE0内)と、CORE1のメロディ再生用の`MELODY_CHANNEL`(コアをまたぐ)。サーバーへ送るデータはチャンネルではなく`outbox.rs`の状態(人ごとの未送信の押下/取り消し、在室の現在値/最後に送信した値)として保持し、単一の`sender_task`が`reqwless::HttpClient`を所有して、Wi-Fi接続中に直列で送信する(ロック不要で、自然にPOSTがレート制限される)。ボタン/在室タスクは`outbox`に記録するだけで決してブロックしない。
- Wi-Fi非依存の動作(`docs/additional_spec.md`「Wi-Fi接続状態に依存しない動作」): ボタン、NeoPixel、オーディオ、照度センサーは、`main.rs`でWi-Fiチップの初期化より前に起動し、Wi-Fiの接続状態に関わらず動作する。Wi-Fiの接続状態はグローバル(`wifi::is_connected()`、リンクアップかつDHCP完了で`true`)で表し、Wi-Fi管理タスク`wifi_task`だけが更新する。サーバーへの送信/同期は、これが`true`の間だけ行う。オフライン中(またはサーバー停止中)の押下は`outbox`に未送信として残り、届くまで再送される。同期(`status_poll.rs`)は未送信の人のLEDを消さない。在室は最後に届けた値と異なる場合だけ、現在値を1回送る。
- オーディオ出力: RP2350の2コア目(CORE1)専属で実行(`audio.rs`、`embassy_rp::multicore::spawn_core1`で起動)— PIO2 + DMA_CH3経由のI2S(`embassy_rp::pio_programs::i2s`、GPIO16=BCLK/GPIO17=LRC/GPIO18=DIN、MAX98357Aアンプ宛)によるリアルタイムDMA供給ループを、CORE0側のネットワーク/LED/ボタン処理の遅延から切り離すため。256サンプルの共有バッファ`WAVEFORM_BUFFER`(`CriticalSectionRawMutex` — embassy-rpのSIOハードウェアスピンロック実装によりコア間で安全)の内容を、CORE1が無限ループで読み出しては`i2s.write().await`し続ける設計。バッファへの書き込みは`waveform.rs`が担い、`i2s.write().await`完了ごとに`Signal`(`audio::BUFFER_CONSUMED`)で駆動されることで実際の再生ペースと1:1で同期している(`docs/additional_spec.md`参照)。
- オーディオ合成: 2 slotのサイン波オシレータ+振幅エンベロープ(attack/release/damp、いずれも「1サンプルごとに目標値へ一定割合で近づく」漸近カーブ)からなる`waveform.rs`のシンセと、5つの固定メロディを`events::MELODY_CHANNEL`(ボタン押下由来のperson_idx)をトリガに再生する`music.rs`の2モジュール構成。サンプリング周波数を意図的に非標準の44,000Hz(=440Hz×100)にすることで、MIDI Note 69(A4=440Hz)がピッチ誤差ゼロで100サンプルの波形テーブルにちょうど一致するようにしてある。attack/release/damp rateとminimum levelは`config.rs`に仮値を置いているだけで実機での聴感調整待ち、5つのメロディ自体もプレースホルダ(`CLAUDE.md`のNotes for future work参照)。

**依存クレートのバージョン**(2026-08-30時点でcrates.ioの実際のリリースに対して検証済みで、`firmware/Cargo.lock`で固定。これらのクレートはバージョンによってAPIの形が変わるため、上げる場合は`~/.cargo/registry/src/`の実ソースと突き合わせて確認すること — 詳細は`CLAUDE.md`の「Notes for future work」を参照):
```
embassy-executor 0.10.0, embassy-time 0.5.1, embassy-rp 0.10.0,
embassy-net 0.9.1 (tcp, dns, dhcpv4), embassy-sync 0.8.0, embassy-usb-logger 0.6.0,
cyw43 0.7.0, cyw43-pio 0.10.0, smart-leds 0.4.0,
reqwless 0.14.0, embedded-io-async 0.7.0,
cortex-m 0.7.9, cortex-m-rt 0.7.6, panic-halt 1.0.0,
static_cell 2.1.1, heapless 0.9.3, log 0.4.34, portable-atomic 1.15.0 (critical-section)
```

**モジュール構成:**
- `main.rs` — エグゼキュータのセットアップ、ハードウェア初期化、全タスクのスポーン。起動順は、(1) NeoPixel/オーディオ/在室/ボタンのタスク(Wi-Fiに依存しない)、(2) `net::init`(Wi-Fiチップとスタックの初期化)、(3) `wifi_task`/`sender_task`/`status_poll_task`
- `secrets.rs` / `secrets.rs.example` / `config.rs` — 上記の通り
- `irqs.rs` — `bind_interrupts!`で`PIO0_IRQ_0`/`PIO1_IRQ_0`/`PIO2_IRQ_0`/`DMA_IRQ_0`/`USBCTRL_IRQ`のハンドラを一箇所にまとめて定義(embassy-rpでは全DMAチャンネルが`DMA_IRQ_0`に固定で、チャンネルごとのIRQ選択はできない)
- `events.rs` — `LedEvent { Pressed{person_idx}, Cancelled{person_idx}, Sync{pressed} }`(NeoPixel反映用)と共有`Channel`の`LED_CHANNEL`(CORE0内で完結)。加えて`MELODY_CHANNEL`(`Channel<CriticalSectionRawMutex, usize, 8>`)— CORE0のボタンタスクからCORE1の`audio.rs`へ、押されたボタンのperson_idx(取り消し時は`CANCEL_MELODY`=5)を渡す、ここで唯一コアをまたぐチャンネル。サーバー送信用のデータはここではなく`outbox.rs`にある
- `debounce.rs` — `buttons.rs`/`occupancy.rs`で共有する`Debouncer`: `wait_for_any_edge()`で変化を検知し、指定時間後もレベルが変わったままなら確定とみなす(チャタリングは静かに無視する)
- `net.rs` — cyw43とembassy-netスタックの初期化のみ(Wi-Fiへの接続はしない)。`(Stack, cyw43::Control)`を返し、`Control`は`wifi_task`に渡される。`Stack`は接続前から作成でき、DHCPはリンクが上がれば自動で進む
- `wifi.rs` — Wi-Fi管理。`cyw43::Control`を所有する`wifi_task`が、`join`(指数バックオフ付きリトライ、一時的なAPの不調から自力で復旧できる必要がある — 動かなくなった基板をデバッグするプローブがないため)、接続後のリンク/DHCP状態の監視、切断時の再`join`を行い、その結果をグローバルの`is_connected()`(`AtomicBool`)に反映する。未接続→接続済みになるたびに、`WIFI_UP`(`status_poll.rs`用)と`outbox::WORK`(`sender_task`用)を`Signal`する。オンボードLED(`WL_GPIO0`、Pico 2 WのオンボードはRP2350のGPIOではなくCYW43439のGPIO0で、`Control`経由でしか制御できず、`join`も同じ`Control`を`&mut`で使うためこのタスクが兼ねる)は`join`の結果を表示する。`join`実行中は点灯を保持し(`join`は完了まで戻らないうえcyw43側にタイムアウトがないため、`config::WIFI_JOIN_TIMEOUT_SECS`(20秒)を超えたら打ち切って失敗として扱い、`leave`してから再試行する)、成功後の接続監視中は`config::ONBOARD_LED_BLINK_HALF_PERIOD_MS`(500ms)ごとに反転して1Hzで点滅し、失敗後のバックオフ待ちの間は「ピピ」(`ONBOARD_LED_FAIL_FLASH_MS`=100msの点灯/消灯を2回、`ONBOARD_LED_FAIL_PATTERN_GAP_MS`=1600msの消灯を挟んで繰り返す)を出して、次の`join`で点灯に戻る。CORE0のエグゼキュータ上で動くため、CORE0が停止/パニックするとLEDの動きが止まり、生存確認になる。CORE1(オーディオ)やサーバー通信の成否は反映しない
- `outbox.rs` — サーバーへ届けるべきものの状態。押下/取り消しは人ごとに「状態語(操作のたびに増えるカウンタ+取り消しフラグ)」と「サーバーが受理した状態語」(不一致=未送信で、フラグが押下か取り消しかを表す。送信中に操作されても取りこぼさず、最後の操作だけが送られる)、在室は「最新の確定値」と「最後に届けた値」。`mark_press`/`mark_cancel`/`set_occupancy`は`WORK`(`Signal`)を発行するだけで決してブロックしない。`next()`が次に届けるものを返し、`complete()`で完了にする
- `buttons.rs` — `Debouncer`を使った5個の非同期タスク(pool_size=5)、ピンごとに50msでデバウンスし、押下確定後、`config::BUTTON_LONG_PRESS_MS`(2000ms)以内に離されたら(`with_timeout`で`Debouncer::debounce`を待つ形)押下として`outbox::mark_press`(サーバー送信用の記録、非ブロッキング)、`LedEvent::Pressed`(LED点灯用)、`MELODY_CHANNEL`へのperson_idx送信(そのボタンに対応するメロディの再生トリガ、`docs/additional_spec.md`参照)を行う。離されないまま経過したら取り消しとみなし(押下メロディは鳴らさない)、`outbox::mark_cancel`、`LedEvent::Cancelled`(LED消灯)、`MELODY_CHANNEL`への`CANCEL_MELODY`送信を行って、離すまで待つ
- `occupancy.rs` — 照度センサーの読み取り(要件通りデジタル`Input`による2値信号)、`Debouncer`でしきい値付近のチャタリングを排除(約2000ms)してから`outbox::set_occupancy`で最新値を記録
- `http_client.rs` — 単一の`reqwless::HttpClient`を所有する`sender_task`が、`outbox::WORK`で起こされ、`wifi::is_connected()`の間だけ`outbox::next()`の内容を`/api/press`・`/api/cancel`に`{"person":"grandpa"}`、`/api/occupancy`に`{"occupied":true}`としてPOSTする。1リクエストは`config::SERVER_REQUEST_TIMEOUT_SECS`(10秒)でタイムアウトする。2xxなら完了、4xx(人物ID不一致など)はログに残して破棄、それ以外(接続失敗、タイムアウト、5xx)は未送信のまま`config::SEND_RETRY_INTERVAL_SECS`(5秒)後に再試行する
- `led.rs` — WS2812 NeoPixelチェーン(PIO1 + DMA_CH2、GPIO15、`config::PEOPLE`と同順)を所有。`LED_CHANNEL`を処理し、`Pressed`で即時点灯、`Sync`(`status_poll.rs`から)で全灯を一括反映 — サーバーの日次リセット後にLEDを消す役目もこれが担う
- `status_poll.rs` — Wi-Fi接続中に`GET /api/led-state`を`config::LED_SYNC_INTERVAL_SECS`ごと(および接続が復帰した直後)にポーリングし、結果を`LedEvent::Sync`として`LED_CHANNEL`にプッシュ(リクエストは`SERVER_REQUEST_TIMEOUT_SECS`でタイムアウト)。サーバーがまだ知らない操作(`outbox`で未送信の押下/取り消し)がある人は、サーバーの答えではなくその操作が求める状態(押下なら点灯、取り消しなら消灯)のままにする。これがサーバー側の日次リセットや、ファームウェア再起動後の状態復元を反映させる仕組み
- `audio.rs` — CORE1専属でMAX98357A向けI2S出力(PIO2 + DMA_CH3、GPIO16=BCLK/GPIO17=LRC/GPIO18=DIN、`embassy_rp::pio_programs::i2s`)を行う。`embassy_rp::multicore::spawn_core1`でCORE1を起動し、`core1_task`(256サンプルの共有バッファ`WAVEFORM_BUFFER`を無限ループで読み出し`i2s.write().await`し続ける)、`waveform::waveform_task`、`music::music_task`の3つをその上にスポーンする。`core1_task`は`i2s.write().await`完了ごとに`Signal`(`BUFFER_CONSUMED`)を発行し、`waveform_task`はこれを待ってから次のチャンクを合成・書き込みすることで、実際の再生ペースと1:1で同期する
- `waveform.rs` — 「波形出力モジュール」(`docs/additional_spec.md`)。100サンプルのサイン波テーブル(`AUDIO_SAMPLE_RATE_HZ`=44,000Hzで、MIDI Note 69=A4=440Hzがピッチ誤差ゼロでちょうど1周する長さ)と、MIDIノート番号ごとの固定小数点(Q16)位相増分テーブル`PITCH_STEP_Q16`(`2^((pitch-69)/12)`から生成)を持つ。2つのslot(`Slot`構造体、状態はIdle/Attack/Release/Damp)を`waveform_task`が所有し、`SLOT_CHANNELS[0]`/`[1]`(`music.rs`から送られる`NoteOn`)を受けて発音、`BUFFER_CONSUMED`で駆動されるたびに`AUDIO_BUFFER_SAMPLES`分を1サンプルずつ合成・加算(オーバーフロー時は`i16`範囲にクランプ)して`WAVEFORM_BUFFER`に書き込む
- `music.rs` — 「音楽データ再生モジュール」(`docs/additional_spec.md`)。`config::PEOPLE`と同順の5つの固定メロディ(`NoteEvent{time, pitch, volume, duration}`の配列、現状はプレースホルダの短い旋律)を持つ`music_task`が、`events::MELODY_CHANNEL`から受け取ったperson_idxでメロディを選び直し、`embassy_time::with_timeout`で次のノートの時刻まで待機しながら`waveform::SLOT_CHANNELS`の2つに交互に`NoteOn`を送出する。再生中に新しいイベントが来たら、鳴っている音は特に消音せずに新しいメロディの先頭から再生を始める

**ビルド/書き込み(移行後のRP2350/Pico 2 W):** `.cargo/config.toml`で`target = "thumbv8m.main-none-eabihf"`、`runner = "picotool load -u -v -x -t elf"`を設定(`elf2uf2-rs`のUF2出力はRP235xでは動作しないことをembassy-rs/embassy#4322で確認済み)。リンカには`flip-link`を使用(スタックオーバーフローのガードページ — デバッガがない環境では有用。このターゲットでも問題なくリンクできることを確認済み)。`memory.x`はRP2350のブートレイアウト(RP2040のBOOT2領域の代わりに`.start_block`/`.bi_entries`/`.end_block`セクション)を使用しています — `embassy-rp`の`rp235xa`featureが必要な`IMAGE_DEF`ブートブロックを自動的に生成してくれるため、`main.rs`側の変更は不要でした。`cargo run --release`でビルド → BOOTSELモードのPico 2 Wに`picotool`経由で書き込みます。

## サーバー(`server/`、Python + Flask)

**構成:** `Flask==2.2.5`(`Werkzeug==2.2.3`、Jinja2を同梱)に加え、本番用WSGIサーバーとして`waitress==2.1.2`を使用 — このアプリはRaspberry Pi 4上でsystemd経由で24時間365日稼働するため(`docs/deploy.md`参照)、実際にデプロイされるのは`wsgi.py`/waitressの組み合わせです。本番のPiはPython 3.7.3(Raspberry Pi OS Buster)のため、これらは3.7をサポートする最後のリリースに固定してあり、コード側も`from __future__ import annotations`で3.8以降専用の型ヒント構文(`X | Y`、`list[X]`)を避けています(`docs/deploy.md`参照)。`app.py`のFlask開発用サーバー(`python app.py`)はローカル開発用としてのみ残しています。永続化は標準ライブラリの`sqlite3`を使用(スキーマは小さなテーブル2つのみで、ORMは不要)。

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
| POST | `/api/cancel` | `{"person": "<id>"}` → 検証は`/api/press`と同じ。`press_status`の`last_pressed_at`/`last_pressed_date`をNULLに戻す(冪等。ボタン長押しによる取り消し) |
| POST | `/api/occupancy` | `{"occupied": true\|false}` → `occupancy_log`に追記(サーバー側での重複排除はせず、ファームウェア側のデバウンスを信頼する) |
| GET | `/api/status` | JSONスナップショット: 人物ごとの`pressed_today`/`last_pressed_at`、現在の`occupied`/`occupied_since` |
| GET | `/api/led-state` | `config.PEOPLE`と同順で1人1文字(押下済みなら`1`、そうでなければ`0`)の`"10100"`形式を`text/plain`で返す — ファームウェアがNeoPixelを同期するための軽量な表現(`docs/protocol.md`参照) |
| GET | `/` | `/api/status`と同じ`db.get_status()`のデータを使って`templates/status.html`を描画 |

`db.py`は`init_db()`、`get_status()`、`get_led_state()`、`record_press()`、`cancel_press()`、`record_occupancy()`を公開しており、ルート側は薄いまま(パース → 呼び出し → 返却/描画)にしています。`get_led_state()`は`get_status()`の結果から組み立てており、`/`・`/api/status`・`/api/led-state`はすべて同じ`get_status()`を情報源にしています(クエリ処理を各ルートに重複させない)。戻り値の型は`models.py`のdataclass(`PersonStatus`/`Status`)、設定値(`PEOPLE`、ホスト/ポート、DBパス)は`config.py`にあります。

**ページ:** `templates/base.html` + `status.html`、`<meta http-equiv="refresh" content="5">`(自宅LAN向けのダッシュボードなのでJS/WebSocketは不要) — 5人分を色付きのピル(押した/押していない)で表示し、在室状態のバナーを1つ表示(在室中/不在 + 開始時刻)。`static/style.css`で最小限のスタイリングを行います。

## 作成されたファイル
```
firmware/Cargo.toml, Cargo.lock, .cargo/config.toml, memory.x, build.rs
firmware/cyw43-firmware/   (cyw43のファームウェア/CLM/nvramのBLOB)
firmware/src/{main,secrets.rs.example,config,irqs,events,debounce,net,wifi,outbox,buttons,occupancy,http_client,led,status_poll,audio,waveform,music}.rs
                            (secrets.rsは実際のWi-Fi情報を入れるgitignore対象のファイル)
server/{requirements.txt,config.py,schema.sql,db.py,models.py,app.py,wsgi.py}
server/deploy/bath-monitor.service
server/templates/{base,status}.html, server/static/style.css
server/tests/{conftest.py,test_api.py}
server/data/.gitkeep   (bath_monitor.dbは実行時状態のためgitignore対象)
docs/{design,additional_spec,protocol,wiring,deploy}.md
schematic/             (KiCadの回路図/基板データ)
README.md, .gitignore  (firmware/target/, firmware/src/secrets.rs, __pycache__/, .venv/, server/data/*.db, .pytest_cache/, schematic/bath-monitor-backups, .DS_Store)
```

## 検証手順
**サーバー(ハードウェア不要):**
```bash
cd server && python -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt
python app.py
# 別のターミナルで:
curl -X POST localhost:8080/api/press -H 'Content-Type: application/json' -d '{"person":"grandpa"}'
curl -X POST localhost:8080/api/cancel -H 'Content-Type: application/json' -d '{"person":"grandpa"}'
curl -X POST localhost:8080/api/occupancy -H 'Content-Type: application/json' -d '{"occupied":true}'
curl localhost:8080/api/status
open localhost:8080/
```
併せて確認すること: 未知の人物 → 400、不正なリクエストボディ → 400、sqliteファイル内の`last_pressed_date`を手動で過去日付にしてリセット処理を確認し、`/api/status`が`false`に切り替わることを確認する。`pytest server/tests/`を実行する。

**ファームウェア(書き込み後):** PicoをBOOTSELモードにして`cargo run --release` → USBシリアル端末を開く(`screen /dev/tty.usbmodemXXXX 115200`) → Wi-Fi接続とIPのログを確認し、その後、物理的なボタン押下によってサーバー側で`POST /api/press 200`が発生すること(Flaskのリクエストログで確認可能)、ダッシュボードのピルが1回の更新サイクル以内に緑色に切り替わることを確認する。照度センサーを覆う/覆いを外すを行い、実際の状態遷移1回につき、チャタリングによる大量発生ではなく、デバウンス済みの`occupancy_log`の行がちょうど1行だけ記録されることを確認する。オンボードLEDは、cyw43の初期化後にWi-Fi接続試行(`join`)中は点灯し、接続に成功したら約1Hzで点滅すること(サーバーを止めても点滅が続くこと)、接続に失敗したら(`secrets.rs`のSSIDをわざと間違えるなど)「ピピ」と短く2回光ることを繰り返し、次の試行で点灯に戻ることを確認する。Wi-Fi非依存の動作として、`secrets.rs`のSSIDをわざと間違えて(またはルーターを止めて)Wi-Fiに接続できない状態にしても、ボタンでNeoPixelが点灯しメロディが鳴ること、その状態で押した後にWi-Fiを復旧させると自動で再接続し、押した内容がサーバーに反映されLEDも消えないこと、接続中にルーターを止めて戻すと切断後に自動で再接続すること、Wi-Fiは生きたままサーバーだけ止めて押しても、サーバー復旧後に反映されることを確認する。ボタンを押して2秒以内に離すと、該当する人物のNeoPixelが点灯してメロディが鳴る(押している間は鳴らない)ことも確認する。長押し(約2秒)では、押下メロディは鳴らずにNeoPixelが消えて取り消し音だけが鳴り、サーバー側で`POST /api/cancel 200`が発生してダッシュボードが「Not yet」に戻ること、離したときに新たな押下として扱われないこと、Wi-Fi未接続で取り消して復旧させるとLEDが再点灯せず、サーバーにも反映されることを確認する。オーディオ(`audio.rs`/`waveform.rs`/`music.rs`)は、各ボタンを押してそれぞれ異なるメロディがスピーカーから鳴ること、再生中に別のボタン(または同じボタン)を押すと途中から新しいメロディに切り替わること、音割れがないことを確認する(メロディはプレースホルダで、attack/release/damp rateやminimum levelも仮値のため、聴感による調整もここで行う)。スピーカー接続前は、ロジックアナライザ等でGPIO16/17(BCLK/LRCLK)に常時クロックが出ていること、ボタン押下時にGPIO18(DIN)へ波形が出ることを確認する。
