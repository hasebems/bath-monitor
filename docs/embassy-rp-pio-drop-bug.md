# embassy-rp 0.10.0: 複数のPIOを使うと、PIOのピンが勝手に「機能なし」に戻る

Raspberry Pi Pico 2 W(RP2350)で、複数のPIOブロックを同時に使うと、embassy-rp 0.10.0 の内部管理の不具合で、PIOに割り当てたピンが、実行の途中で黙って切り離されることがあります。この文書は、実際にそれに当たったときの調査の記録と、上流の修正がリリースされるまでの回避策をまとめたものです。同じ症状に当たった人が、原因の切り分けを短縮できることを目的にしています。

- 対象: `embassy-rp` 0.10.0(確認したのはこのバージョンのソースです)、RP2350(Pico 2 W)。RP2040でも、同じコードを使っているので起こりうると思いますが、確認していません
- 状況の確認日: 2026-09-19

## 要約

- 症状: PIOで駆動するはずのGPIOに、信号がまったく出ない。ソフトのログ上は、PIOもDMAも正常に動いている。
- 原因: `embassy-rp` が、PIOの「利用者数」と「使用中のピン」を記録する変数が、PIO0・PIO1・PIO2で共有されていた。ある `Pio` の未使用の部品をdropしただけで、別のPIOに割り当てたピンまで、機能なし(NULL)に戻される。
- 回避策: 未使用の `Common` と `StateMachine` を、dropさせずに `core::mem::forget` で捨てる。
- 上流の状況: [embassy-rs/embassy#5714](https://github.com/embassy-rs/embassy/pull/5714)「Fix PIO drop」が、2026-04-07 にマージ済みです。ただし、0.10.0(2026-03-20 公開)より後の修正で、2026-09-19 時点の crates.io の最新版も 0.10.0 なので、リリース版にはまだ入っていません。

## 環境

浴室の使用状況を記録する家庭用IoT機器で、Pico 2 W を次のように使っています。

| PIO | 用途 | ピン |
|---|---|---|
| PIO0 | CYW43439(Wi-Fi)とのSPI(`cyw43-pio`) | 内蔵(GPIO24、25、29) |
| PIO1 | WS2812(NeoPixel)5個(`embassy_rp::pio_programs::ws2812`) | GPIO15 |
| PIO2 | I2Sオーディオ出力(CORE1で動作) | GPIO16、17、18 |

- executorは `embassy_rp::executor::Executor`(CORE0)で、オーディオだけCORE1で動かしています。
- 主なクレートは `embassy-rp` 0.10.0、`cyw43` 0.7.0、`cyw43-pio` 0.10.0、`embassy-executor` 0.10.0、`embassy-time` 0.5.1 です。

## 症状

- NeoPixelのデータピン(PIO1)で、信号が出ない。オシロで見ると、0Vのままだったり、中間電位(2V前後)だったりして、状況が一定しない。
- USBシリアルのログでは、NeoPixelのタスクは正常に動いていた。起動し、PIOの初期化を終え、100msごとに `write` がDMA完了まで戻っている。
- ピンをGPIO15からGPIO14に変えても、まったく同じ症状だった。
- ボード自体は正常で、Wi-Fiも、オーディオ(PIO2)も動いていた。

## 調査の経過

調査は長くなりました。原因が2つ重なっていたためです(後述)。

1. ハードウェア(ブレッドボード、レベルシフタ、プローブ)を疑った。ピンを変えても症状が変わらなかったので、ソフト側を疑うことにした。
2. ソフトを、公式の `pio_ws2812` サンプルと突き合わせた。呼び出しの順序、割り込みの結線、DMAの要求番号、クロック分周、`GPIOBASE` を確認したが、違いはなかった。
3. NeoPixelが「イベントのときにだけ」信号を出す作りだと、オシロで捉えにくいので、10fpsで毎フレーム書き込む作りにした。それでも、症状は変わらなかった。
4. **決め手: ピンの状態を、レジスタから直接ログに出した。** これで、初めてはっきりした(次の節)。

## 決め手: レジスタを読むログ

`embassy-rp` の `unstable-pac` フィーチャを有効にして、ピン自体のレジスタを1秒ごとにシリアルへ出しました。

```rust
use embassy_rp::pac;

let pin = 14usize; // 対象のGPIO
let status = pac::IO_BANK0.gpio(pin).status().read();
let funcsel = pac::IO_BANK0.gpio(pin).ctrl().read().funcsel(); // 7 = PIO1、31 = NULL
let padoe = (pac::PIO1.dbg_padoe().read() >> pin) & 1;           // PIO1から見た出力有効
log::info!(
    "GPIO{} funcsel={} oe={} out={} in={} | PIO1 padoe={}",
    pin, funcsel,
    status.oetopad() as u8, status.outtopad() as u8, status.infrompad() as u8,
    padoe,
);
```

出力の抜粋です(実際のログを、要点だけに絞っています。この時は、切り分けのために、データピンをGPIO14に変えて試していました)。

```
led: 10 frames written
led: GPIO14 funcsel=7  oe=1 out=1 in=1 | PIO1 padoe=1
network stack ready (not yet joined to Wi-Fi)
led: 20 frames written
led: GPIO14 funcsel=31 oe=0 out=0 in=0 | PIO1 padoe=1
```

- 起動の約1秒後は、`funcsel=7`(PIO1)で、ピンは出力になっている。
- Wi-Fiチップの初期化(`net::init`)が終わった後は、`funcsel=31`(NULL)、`oe=0` に変わっている。**その後、二度と戻らない。**
- PIO1のほうは、`padoe=1` のまま、出力しているつもりでいる。パッドの機能だけが、PIOから切り離されている。
- パッドの設定レジスタ(プルや入力バッファ)は変わっていない。書き換えられたのは、機能選択(`funcsel`)だけだった。

「ソフトは正常に動いているのに、パッドがPIOにつながっていない」ことが、はっきり分かります。この「機能選択だけをNULLに戻す」動作は、`embassy-rp` の `on_pio_drop` が行うものと一致しました。

## 原因

`embassy-rp` 0.10.0 の `pio/mod.rs` では、PIOの状態が次のように保持されていました(要点のみ)。

```rust
trait SealedInstance {
    // ...
    fn state() -> &'static State {
        static STATE: State = State {
            users: AtomicU8::new(0),        // 利用者数(Common 1 + StateMachine 4)
            used_pins: AtomicU64::new(0),   // 使用中のピンの集合
        };
        &STATE
    }
}
```

これは、PIOごとに別々の状態を持つつもりの書き方です。しかし、Rustでは、ジェネリックな関数(トレイトのデフォルトメソッドも含む)の中の `static` は、型ごとに複製されず、プログラム全体で1つです。PIO0、PIO1、PIO2が、同じ `State` を共有していました。

小さなコードで確認できます。

```rust
use std::sync::atomic::{AtomicU8, Ordering};
struct State { users: AtomicU8 }
trait Inst {
    fn state() -> &'static State {
        static STATE: State = State { users: AtomicU8::new(0) };
        &STATE
    }
}
struct Pio0; struct Pio1;
impl Inst for Pio0 {}
impl Inst for Pio1 {}
fn main() {
    Pio0::state().users.store(5, Ordering::SeqCst);
    println!("Pio1 sees users = {}", Pio1::state().users.load(Ordering::SeqCst)); // 5
    println!("same address: {}", std::ptr::eq(Pio0::state(), Pio1::state()));     // true
}
```

### 今回、何が起きたか

`Pio::new` は、`users` を 5 に、`used_pins` を空にします。`Common` や `StateMachine` をdropするたびに、`on_pio_drop` が `users` を1つ減らし、**1から0に減った(最後の利用者が消えた)とき、記録にあるすべてのピンの機能をNULLに戻します。**

このファームウェアでは、次の順で起きていました(実際の数値は、CORE1側の起動のタイミングにも左右されます)。

1. `led_task` が `Pio::new(pio1)` を呼ぶ。`users` が 5、`used_pins` が空になる(それまでのPIO0のピンの記録も、消える)。
2. `let Pio { mut common, sm0, .. } = Pio::new(pio1, Irqs);` の `..` で、使わないSMが3つdropされる。`users` が 2 になる。
3. NeoPixelのピンが記録される。`used_pins` には、GPIO15(のちに14)だけが入っている。
4. `net::init` が `let mut pio = Pio::new(pio0, Irqs);` の未使用の部品(`common` と SM 3つ)を、関数を抜けるときにdropする。`users` が 2 から 1、そして 0 に減る。**0に減った瞬間に、記録にあるNeoPixelのピンが、NULLに戻される。**
5. `users` は、その後もさらに減って、アンダーフローする。デバッグビルドだと `subtract with overflow` でパニックする。リリースビルドでは、値が回って、黙って進む。

上流のPR #5714 が報告している症状(「複数のPIOをdropすると、`subtract with overflow` でパニックする」)と、同じ原因です。今回のピンが切り離される症状は、その別の現れ方だと考えています。

補足として、オーディオ(PIO2、GPIO16〜18)が無事だった理由は、`Pio::new` が `used_pins` を空にするため、その記録から外れていた(または記録のタイミングが違った)ためと考えられますが、これは推測で、確認していません。つまり、起動順に依存する、偶然の産物である可能性が高いです。

## 回避策

未使用の `Common` と `StateMachine` を、dropさせずに `core::mem::forget` で捨てます。`Pio::new` を呼ぶ、すべての場所に入れます。

```rust
// net.rs (PIO0)
let Pio { mut common, sm0, irq0, sm1, sm2, sm3, .. } = Pio::new(pio0, Irqs);
let spi = PioSpi::new(&mut common, sm0, RM2_CLOCK_DIVIDER, irq0, cs, dio, clk, dma);
core::mem::forget((common, sm1, sm2, sm3));

// led.rs (PIO1)、audio.rs (PIO2): タスクが終わらないので、common は生かしたまま
let Pio { mut common, sm0, sm1, sm2, sm3, .. } = Pio::new(pio1, Irqs);
core::mem::forget((sm1, sm2, sm3));
```

- ポイントは、`Pio { .., sm0, .. }` の `..` で捨てられる部品も、dropされることです。`sm1`〜`sm3` を明示的に受けて、`forget` する必要があります。
- 副作用はほとんどありません。ファームウェアは、PIOを終了まで使い続けるので、資源の解放が不要です。未使用のSMは、有効にしていないので、動きません。
- **この回避策で、信号が出るようになったことを確認しました。** 書き込み直して、GPIOに信号が出ました。レジスタのログで `funcsel=7` が続くことは、書き込み後に、あらためて取っていません。
- 上流の修正版(`main`)を取り込む方法もあります(`[patch.crates-io]`)。しかし、embassy系のクレートは、互いにバージョンが強く結びついていて、APIの変更に巻き込まれる恐れがあるため、今回は見送りました。

## 上流の状況(2026-09-19 時点)

- [PR #5714「Fix PIO drop」](https://github.com/embassy-rs/embassy/pull/5714)が、2026-04-07 にマージされている。PIOごとに別の `static` を持たせる修正で、上流の `main` の `mod.rs` では、`state()` が `match Self::PIO_NO { 0 => STATE_0, 1 => STATE_1, ... }` の形になっていることを確認した。
- `embassy-rp` 0.10.0 は 2026-03-20 の公開で、この修正を含まない。crates.io の最新版は、確認した時点で 0.10.0 のまま。
- [issue #6015](https://github.com/embassy-rs/embassy/issues/6015)(RP2350で、PIOを2つ以上初期化すると、`main` の終了後にタスクが止まる)は未解決。原因は特定されておらず、今回の件と同じ原因かどうかは、確認していない。
- 上流の `main` を、このファームウェアに取り込んで動かす確認は、していない(上のとおり、見送った)。

## 自分が影響を受けているかの見分け方

次の条件が重なると、疑う価値があります。

- 複数のPIOブロックを使っている(たとえば、cyw43とWS2812、または、cyw43とI2S)。
- どこかで、`Pio` の一部をdropしている。次のような形も含まれます。
  - `let Pio { common, sm0, .. } = Pio::new(...)` の `..` で、使わないSMを捨てている。
  - `Pio::new` の結果を、関数のローカルに持ったまま、関数を抜けている。
- PIOで駆動するピンに、信号が出ない、または、途中から止まる。

確認は、上の「レジスタを読むログ」を入れて、`funcsel` が、動作中にPIOの値(今回のPIO1では 7)から 31(NULL)に変わっていないかを見るのが、確実です。PIO0とPIO2の値は、RP2350のデータシートのGPIO機能表で確認してください。

## 教訓

- **原因が2つ重なっていました。** ここまでの内容は、ソフト側の原因(embassy-rpの不具合)です。これとは別に、NeoPixelのDINとDOUTを、ブレッドボードで逆につないでいました。「ソフトの不具合を直したのに、まだ光らない」状態が、ハードの原因のせいで、続いていました。信号の有無と、光るかどうかは、別々に確かめる必要があります。
- **「ソフトは正常に動いているログ」と「ピンが実際に駆動されている」は、別のことです。** タスクが動き、DMAが完了しても、パッドが切り離されていれば、何も出ません。チップのレジスタを、直接読んで確認するのが、最も早い切り分けでした。
- **`static` を、ジェネリックな関数やトレイトのデフォルトメソッドの中に書くと、型ごとには複製されません。** ライブラリの実装でも、起こりうる、Rustの落とし穴です。

## 付記

この調査と回避策の実装は、AIコーディングアシスタント(Claude Code)との共同作業で行いました。上流の状況の確認は、GitHubのページと、crates.ioの情報を参照して行っています。上に書いたバージョンや日付は、2026-09-19 時点のものです。
