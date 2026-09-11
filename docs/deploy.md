# Raspberry Pi 4上でのサーバーの24時間365日デプロイ

対象: Raspberry Pi OS(Debianベース、systemd)。サーバーは起動時に自動開始し、クラッシュ時には自動再起動するsystemdサービスとして常時稼働させます。サーバーは純粋なPython + 標準ライブラリの`sqlite3`のみで構成されているため、アーキテクチャ固有の手順は不要です。

**Pythonバージョンに関する注意**: このプロジェクトの基準としているPiはRaspberry Pi OS Busterを使用しており、**Python 3.7.3**が動いています。`server/requirements.txt`はPython 3.7をまだサポートしている最後のFlask/Werkzeug/waitressのリリースに固定されており(Flask 2.2.5 — それより新しいFlaskはPython 3.9以上が必要)、コード側も`from __future__ import annotations`によってPython 3.8以降専用の構文(`X | Y`のユニオン型や`list[X]`のような組み込みジェネリクス)を避けています。お使いのPiがより新しいPython(BullseyeやBookwormには3.9/3.11が入っています)を搭載している場合でも、これらのバージョン固定のままで問題なく動作します — OSバージョンごとに特別扱いする必要はありませんでした。

## インストール

```bash
git clone https://github.com/hasebems/bath-monitor.git ~/bath-monitor
cd ~/bath-monitor/server
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

`/home/pi/bath-monitor`以外の場所にcloneする場合や、`pi`以外のユーザーで実行する場合は、以下のパスを適宜調整してください。

## systemdサービス

`server/deploy/bath-monitor.service`がユニットのテンプレートです — 長期間無人稼働させるため、`app.py`のFlask開発用サーバーではなく、`server/wsgi.py`(本番用WSGIサーバーであるwaitress)を実行します。

```bash
sudo cp ~/bath-monitor/server/deploy/bath-monitor.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now bath-monitor
```

状態/ログの確認:
```bash
systemctl status bath-monitor
journalctl -u bath-monitor -f
```

これで、自宅LAN上のどの端末からでも`http://<pi-hostname-or-ip>:8080/`でダッシュボードにアクセスできるようになります — ファームウェア側の`SERVER_BASE_URL`(`firmware/src/secrets.rs`)にそのアドレスを設定してください。

## コード変更後の更新方法

```bash
cd ~/bath-monitor && git pull
cd server && .venv/bin/pip install -r requirements.txt   # requirements.txtが変更された場合のみ
sudo systemctl restart bath-monitor
```

## 補足事項

- `server/data/bath_monitor.db`は再起動をまたいで永続化されます(ローカルディスク上にあり、gitignore対象です)。過去の`occupancy_log`データを残しておきたい場合はバックアップしてください。
- 押下状況の日次リセットは、Piのシステム時計/タイムゾーンが家庭の設定に合っていることに依存します(`date.today()`はローカル時刻を使用します) — 想定と違う時刻にリセットされるようであれば`timedatectl`で確認してください。
- ユニットファイル内の`Restart=always`により、クラッシュや`journalctl`で確認できる例外が発生した場合でも、手動対応なしに`RestartSec=5`以内にサービスが復旧します。
