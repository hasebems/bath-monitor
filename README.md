# bath-monitor

5人家族の我が家では、お風呂に最後に入った人が湯船の水を抜いて洗うことになっています。しかし、その運用には以下の問題があります。
- まだ入ってない人がいるのに、湯船が空になって洗われていた
- もうみんな入ったのに、まだ入っていない人がいると勘違いして、そのまま朝まで湯船が洗われなかった

その後、入った人は磁石で印を付けるようにしたのですが、その運用は徹底されず、たびたびお風呂に関する問題が起き続けています。  

この bath-monitor はそんな家族の問題を解決するために開発しました。  
bath-monitor は以下の機能を持っています。
- 家族ごとに５つのボタンを搭載。ボタンが押されると LED が点灯し、ボタンごとに異なるメロディを鳴らす
- ボタンの押し間違いがあったら、二秒の長押しで LED は消灯し、キャンセルできる
- 照度センサーを搭載し、脱衣所が明るいか暗いかで誰かが入っていることをセンシングする
- ボタンとセンサーの信号はWi-Fi経由で自宅LAN上のFlaskサーバーに送られ、サーバーはライブダッシュボードを表示
- 携帯や PC でダッシュボードにアクセスすれば、誰が入ったか、今誰かが入っているかをリアルタイムに検知可能

設計の詳細は`docs/design.md`、HTTPの契約(プロトコル)は`docs/protocol.md`、ピン割り当ては`docs/wiring.md`を参照してください。embassy-rpのPIOの不具合の調査記録と回避策は`docs/embassy-rp-pio-drop-bug.md`にまとめています。

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

- `XXXX`の部分: Pico 2 WをUSB接続した状態で`ls /dev/tty.usbmodem*`を実行すると確認できる(複数のUSBシリアルデバイスが繋がっている場合は、抜き差し前後で`ls`の差分を見ると確実)
- `screen`の終了方法: `Ctrl-a`の後に`k`を押し、確認プロンプトで`y`を押す(`Ctrl-a` `d`はデタッチのみでプロセスは残るため注意)

## 現在の状況

- サーバーは実機のRaspberry Pi 4上にデプロイ・動作確認済み(ダッシュボードに自宅LAN上のスマートフォンからアクセスできることを確認済み)
- 対象基板は Pico W から Pico 2 W(RP2350) に変更となり、ファームウェアのコードはRP2350向けに移行済みです(`thumbv8m.main-none-eabihf`向けに`cargo clippy`まで通過)
- その後2026-09-19に実機のPico 2 Wへ書き込み、Wi-Fi接続、ボタン(1個目)、MAX98357Aアンプからのメロディ、オンボードLED、サーバーへの送信は動作を確認済み
- 照度センサーは、センサーを覆うとデバウンスされた在室状態が`POST /api/occupancy`で送られ、サーバーのダッシュボードにも在室と表示されることを確認済みです(2026-09-20)
- NeoPixelは、点灯・白の呼吸・長押し取り消しでの消灯を確認済みです。一方、残りのボタン(2〜5個目)と、押した人ごとの色は実機での動作確認がまだです — 確認すべき内容は`docs/design.md`の検証手順の節を参照してください。
- 照度センサーの浴室での明るさ(トリムポットのしきい値)の調整もまだです
- 現在はブレッドボード上での動作確認で、専用の基板を設計中です。基板が完成したら、`docs/design.md`の検証手順に沿って全機能をチェックする予定です
