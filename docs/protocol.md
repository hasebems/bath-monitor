# HTTPプロトコル

`firmware/`(クライアント)と`server/`(Flaskアプリ)間の正式な契約(コントラクト)。両者はこの内容に一致させる必要があります — ファームウェア側の`PEOPLE`/`config.rs`の人物IDは、`server/config.py`の`PEOPLE`リストと完全に一致していなければなりません。

ベースURL: `http://<server-host>:8080`(ファームウェア側は`secrets.rs`の`SERVER_BASE_URL`、サーバー側は`server/config.py`の`HOST`/`PORT`で設定)。

## `POST /api/press`

デバウンス処理済みのボタン押下を、サーバーへ届けるために送信されます。Wi-Fiが切れている間やサーバーが応答しない間の押下は、ファームウェア側で「未送信」として保持され、2xxが返るまで再送されます(同じ人の複数回の押下は1回にまとまります)。サーバー側は同日中の再押下を冪等に扱うため、再送されても問題ありません。4xxが返った場合は、再送しても成功しないため破棄します。

サーバーは受信した時刻を記録するため、オフライン中の押下の記録時刻は、実際の押下時刻ではなく、送信に成功した時刻になります(深夜0時をまたぐと翌日の記録になります)。

リクエストボディ:
```json
{ "person": "grandpa" }
```

レスポンス `200 OK`:
```json
{ "status": "ok", "person": "grandpa", "date": "2026-08-30" }
```

レスポンス `400 Bad Request`(`person`が未知または欠落している場合):
```json
{ "status": "error", "message": "unknown person" }
```

## `POST /api/occupancy`

確定した(デバウンス済みの)在室状態の変化を送信します。Wi-Fiが切れている間やサーバーが応答しない間は、最後にサーバーへ届けた状態と現在の状態が異なる場合に限り、復帰後に現在の状態を1回だけ送ります(オフライン中の途中の変化は送られません)。記録時刻は、送信に成功した時刻になります。

リクエストボディ:
```json
{ "occupied": true }
```

レスポンス `200 OK`:
```json
{ "status": "ok", "occupied": true, "timestamp": "2026-08-30T21:14:03+09:00" }
```

レスポンス `400 Bad Request`(`occupied`が欠落または真偽値でない場合):
```json
{ "status": "error", "message": "occupied must be a boolean" }
```

サーバー側では同じ状態が連続して送られてきても重複排除は行いません — 送信前にデバウンスする責任はファームウェア側にあります(サーバーが受理したのに応答がタイムアウトした場合、ファームウェアが同じ内容を再送するため、同じ状態が2回記録されることがあります)。

## `GET /api/status`

ダッシュボードページで使われるJSONスナップショットで、`curl`や動作確認にも利用できます。

レスポンス `200 OK`:
```json
{
  "people": [
    { "id": "grandpa", "pressed_today": true, "last_pressed_at": "2026-08-30T19:02:11+09:00" },
    { "id": "grandma", "pressed_today": false, "last_pressed_at": null }
  ],
  "occupied": false,
  "occupied_since": "2026-08-30T19:05:00+09:00"
}
```

## `GET /api/led-state`

ファームウェアが定期的に行うNeoPixel同期処理で使われる、組み込み機器向けのコンパクトな状態表現(デバイス上でのJSONパースを避けるため)。`PEOPLE`の順序で1人につき1文字、`pressed_today`なら`1`、そうでなければ`0`。`Content-Type: text/plain`。

レスポンス `200 OK`のボディ(5人のうちgrandpaとfatherが今日押した場合の例):
```
10100
```

ファームウェアはこれを一定間隔でポーリングし(`firmware/src/status_poll.rs`を参照)、5個すべてのNeoPixelをこの状態に合わせて再調整します — これにより、サーバー側の日次リセット後にLEDが再び消灯し、またファームウェア再起動後も正しいLED状態が復元されます。

## `GET /`

`/api/status`と同じデータを使ってHTMLダッシュボードを描画します。
