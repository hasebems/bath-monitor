# bath-monitor

今日誰がお風呂に入ったか?を記録する仕組み。浴室に設置したRaspberry Pi Pico 2 Wが5個のボタン(家族一人につき1個)と人感(照度)センサーを読み取り、ボタンが押されるたびにNeoPixelを点灯させ、MAX98357Aアンプ経由で押したボタンごとに異なるメロディを鳴らします。両方の信号はWi-Fi経由で自宅LAN上のFlaskサーバーに送られ、サーバーはライブダッシュボードを表示します。

設計の詳細は`docs/design.md`、HTTPの契約(プロトコル)は`docs/protocol.md`、ピン割り当ては`docs/wiring.md`を参照してください。

## サーバー

```bash
cd server
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python app.py            # http://<this-machine>:8080
```

テスト: `pytest tests/ -q`(`server/`ディレクトリで、venvを有効化した状態で実行)。

Raspberry Pi上でsystemdサービスとして24時間365日稼働させる方法(`wsgi.py`経由の本番用WSGIサーバー、自動起動・自動再起動)は`docs/deploy.md`を参照してください。

## ファームウェア

`rustup target add thumbv8m.main-none-eabihf`(RP2350はArm Cortex-M33のため)と、書き込み用に`picotool`をPATHに通しておく必要があります(デバッグプローブ不要 — 書き込みはBOOTSEL + `picotool`で行います。`elf2uf2-rs`はRP2350/RP235xでは動作しません)。

```bash
cp firmware/src/secrets.rs.example firmware/src/secrets.rs   # 実際のWi-Fi/サーバー情報を記入
cd firmware
cargo build --release     # コンパイルチェックのみ
cargo run --release       # ビルドし、BOOTSELモードのPico 2 Wにpicotool経由で書き込み
```

ログはUSBシリアル経由(`screen /dev/tty.usbmodemXXXX 115200`)で確認します(RTT/defmtではありません)。

## 現在の状況

サーバーは実機のRaspberry Pi 4上にデプロイ・動作確認済みです(ダッシュボードに自宅LAN上のスマートフォンからアクセスできることを確認済み)。対象基板はファームウェアを実機に書き込む前にPico WからPico 2 W(RP2350)に変更となり、ファームウェアのコードは同日中にRP2350向けに移行済みです(`thumbv8m.main-none-eabihf`向けに`cargo clippy`まで通過)が、実機のPico 2 Wへはまだ書き込んでいないため、ボタン/照度センサー/NeoPixel/MAX98357Aアンプ/Wi-Fi接続は実機での動作確認がまだできていません — 配線後に確認すべき内容は`docs/design.md`の検証手順の節を参照してください。
