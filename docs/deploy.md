# Raspberry Pi 4上でのサーバーの24時間365日デプロイ

対象: Raspberry Pi OS(Debianベース、systemd)。サーバーは起動時に自動開始し、クラッシュ時には自動再起動するsystemdサービスとして常時稼働させます。サーバーは純粋なPython + 標準ライブラリの`sqlite3`のみで構成されているため、アーキテクチャ固有の手順は不要です。

**Pythonバージョンに関する注意**: このプロジェクトの基準としているPiはRaspberry Pi OS Busterを使用しており、**Python 3.7.3**が動いています。`server/requirements.txt`はPython 3.7をまだサポートしている最後のFlask/Werkzeug/waitressのリリースに固定されており(Flask 2.2.5 — それより新しいFlaskはPython 3.9以上が必要)、コード側も`from __future__ import annotations`によってPython 3.8以降専用の構文(`X | Y`のユニオン型や`list[X]`のような組み込みジェネリクス)を避けています。お使いのPiがより新しいPython(BullseyeやBookwormには3.9/3.11が入っています)を搭載している場合でも、これらのバージョン固定のままで問題なく動作します — OSバージョンごとに特別扱いする必要はありませんでした。

## インストール

```bash
mkdir -p ~/xxx && cd ~/xxx
git clone https://github.com/hasebems/bath-monitor.git
cd ~/xxx/bath-monitor/server
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

この手順では、リポジトリを`~/xxx/bath-monitor`(`/home/pi/xxx/bath-monitor`)に置くものとします。`xxx`は実際に置くフォルダ名に読み替えてください。`pi`以外のユーザーで実行する場合も、以下のパスを適宜調整してください。

## 手動での起動(動作確認)

サービスとして登録する前に、手動で起動して動くことを確かめられます。本番用の起動は`wsgi.py`です(下記のsystemdサービスが実行するのと同じコマンドです)。

```bash
cd ~/xxx/bath-monitor/server
.venv/bin/python wsgi.py           # http://<pi-hostname-or-ip>:8080/
```

`Serving on http://0.0.0.0:8080`と表示されれば起動しています。確認したらCtrl+Cで止めてください(動かしたままだと8080番ポートがふさがり、サービスが起動できません)。

`python app.py`でも起動できますが、こちらはFlask付属の開発用サーバーで、起動時に「This is a development server. Do not use it in a production deployment.」という警告が出ます。開発用のサーバーは手元での確認向けで、長時間の稼働には向かないため、Pi上では`wsgi.py`を使います。`app.py`はアプリ本体(ルートの定義)で、`wsgi.py`はそれを本番用WSGIサーバーのwaitressで起動するだけのファイルです。

## systemdサービス

`server/deploy/bath-monitor.service.example`がユニットのサンプルです。これを`server/deploy/bath-monitor.service`(gitの管理対象外)にコピーし、パスを自分の環境に合わせて書き換えてから使います(`firmware/src/secrets.rs.example`と同じ扱い)。このユニットは、長期間無人稼働させるため、`app.py`のFlask開発用サーバーではなく、`server/wsgi.py`(本番用WSGIサーバーであるwaitress)を実行します。

```bash
cd ~/xxx/bath-monitor/server/deploy
cp bath-monitor.service.example bath-monitor.service
nano bath-monitor.service          # WorkingDirectory/ExecStartの xxx を実際のフォルダ名に、BATH_MONITOR_NAME_* を家族の表示名に書き換える
sudo cp bath-monitor.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now bath-monitor
```

状態/ログの確認:
```bash
systemctl status bath-monitor
journalctl -u bath-monitor -f
```

これで、自宅LAN上のどの端末からでも`http://<pi-hostname-or-ip>:8080/`でダッシュボードにアクセスできるようになります — ファームウェア側の`SERVER_BASE_URL`(`firmware/src/secrets.rs`)にそのアドレスを設定してください。

### ダッシュボードの表示名

ダッシュボードに出す5人の名前は、ユニットファイルの`[Service]`にある`Environment=BATH_MONITOR_NAME_<ID>=<表示名>`の行で設定します(`<ID>`は`server/config.py`の`PEOPLE`のIDを大文字にしたもの。例: `Environment=BATH_MONITOR_NAME_GRANDPA=おじいちゃん`)。行が無い人や値が空の人は、ID(`grandpa`など)のまま表示されます。名前はダッシュボードの表示にだけ使い、ファームウェアとのやり取りや`/api/*`の応答には影響しません。

表示名を変えたら、ユニットファイルをコピーし直して再起動します(名前はサーバーの起動時に読むため):

```bash
cd ~/xxx/bath-monitor/server/deploy
nano bath-monitor.service
sudo cp bath-monitor.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl restart bath-monitor
```

`python app.py`/`python wsgi.py`を直接起動したときは、環境変数を設定しない限りIDのまま表示されます(例: `BATH_MONITOR_NAME_GRANDPA=おじいちゃん python wsgi.py`)。

## コード変更後の更新方法

```bash
cd ~/xxx/bath-monitor && git pull
cd server && .venv/bin/pip install -r requirements.txt   # requirements.txtが変更された場合のみ
sudo systemctl restart bath-monitor
```

`bath-monitor.service.example`が更新された場合(表示名の`Environment=`の行の追加など)、自分の`bath-monitor.service`には自動では反映されません。差分を見て必要な行を書き足し、上の「ダッシュボードの表示名」と同じ手順でコピーし直してください。

## 補足事項

- `server/data/bath_monitor.db`は再起動をまたいで永続化されます(ローカルディスク上にあり、gitignore対象です)。過去の`occupancy_log`データを残しておきたい場合はバックアップしてください。
- 押下状況の日次リセット(ローカル時刻の朝5時、`server/config.py::DAY_START_HOUR`)は、Piのシステム時計/タイムゾーンが家庭の設定に合っていることに依存します(`datetime.now()`はローカル時刻を使用します) — 想定と違う時刻にリセットされるようであれば`timedatectl`で確認してください。
- ユニットファイル内の`Restart=always`により、クラッシュや`journalctl`で確認できる例外が発生した場合でも、手動対応なしに`RestartSec=5`以内にサービスが復旧します。
