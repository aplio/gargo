# TODO

**進捗（2026-07-31）**: A 系・B 系すべて実装済み。残りは C 系のみ。

A-4 は druk / tokyonight 等からの取り込みはせず、gargo 独自パレット 3 本
（`gargo_dim` / `gargo_contrast` / `gargo_sepia`）を書き下ろした（ライセンス確認が不要なため）。

**C-1 の事前確認の結果（済）**: `src/input/keymap.rs` は単一の情報源に **なっていない**。
1. `src/ui/overlays/command_helper.rs` が SPC / g チョードの一覧を**手書きで二重管理**している。
   実際に既にズレていて、`SPC /`（グローバル検索）は keymap にあるのにヘルパーに出ない。
2. 各オーバーレイが自前でキーを直接 match している（sidebar 68 / popup 61 /
   commit_log 40 / pr_picker 38 箇所）。テーブルがないので「今このペインで何が押せるか」は
   そもそも導出できない。
→ C-1 をやるなら本体は「キー定義テーブルの導入」で、規模は中〜大。

druk（Bun + SolidJS + OpenTUI の TUI エディタ）の設計調査から切り出したタスク。
コードは移植できない（Rust / crossterm と JS / OpenTUI）が、**設計は移植できる**。

各タスクは独立して着手できるよう、対象ファイル・行・完了条件を書いてある。
着手前に必ず「判断が必要なもの」を先に決めること — 決めずに始めると手戻りする。

---

## 判断が必要なもの（着手前に決める）

### D-1. truecolor 非対応ターミナルをどう扱うか → **A-2 の前提**

druk は OpenTUI 前提で truecolor を無条件に仮定している。gargo は crossterm で
素の端末に出すので、hex プリセットを入れると非対応端末で色が壊れる可能性がある。

- **(a) truecolor 前提。非対応端末は考慮しない** — 実装は最小。2026 年の主要端末は
  ほぼ対応済み。ただし tmux 設定次第で落ちる環境が残る
- **(b) `COLORTERM=truecolor|24bit` を見て、非対応なら ANSI 16 色にフォールバック** —
  `Theme` に「hex とその ANSI 近似」の両方を持たせる。プリセット定義のコストが倍
- **(c) hex プリセットと ANSI プリセットを別物として並べ、ユーザーが `preset` で選ぶ** —
  実装は (a) とほぼ同じで、既存の `ansi_dark` / `ansi_light` を残すだけ

推奨: **(c)**。既存プリセットを消さずに済み、フォールバック判定のロジックが要らない。
(b) が欲しくなったら後から「`preset = "auto"`」を足せばよい。
=> cでok

### D-2. `Explorer` にどうやって設定を渡すか → **B-1 の前提**

`Explorer` は現状 `Config` を一切持たない（`Explorer::new(dir, project_root, git_status_map)`、
`src/ui/overlays/explorer/sidebar.rs:183`）。しかも `read_directory()` が
コンストラクタ内で走るので、「作ってから setter」では初回描画に間に合わない。

- **(a) `Explorer::new` の引数に足す** — 素直。ただし呼び出し側が本番 5 箇所
  （`src/app/open_click.rs:110`, `src/app/dispatch_app.rs:475/491/501/515`）＋
  **テスト約 27 箇所**（`sidebar.rs` 内 25、`compositor/tests.rs` 2）で全部書き換え
- **(b) `FileFilter` 構造体を作って `Default` を実装し、`new_with_filter` を追加。
  既存 `new` は `FileFilter::default()` で委譲** — テストは無変更、本番だけ差し替え
- **(c) `Explorer` に `show_dotfiles: bool` フィールドを足し、`Default` は `true`。
  コンストラクタは可変長にせず、本番側は `new` 直後に `set_show_dotfiles()` →
  `read_directory()` を明示的に呼ぶ** — 二度読みが発生する

推奨: **(b)**。テスト 27 箇所を触らずに済み、B-3 で gitignore を足すときも
`FileFilter` にフィールドを 1 つ生やすだけで済む。
=> bでok

### D-3. `show_dotfiles` のデフォルト値

- **(a) `true`（＝ドットファイルを表示）** — druk と同じ。現状の gargo の挙動が変わる
- **(b) `false`（＝現状維持）** — 既存ユーザーに影響なし。ただし「見えない」問題は
  設定を書いた人しか解決しない

推奨: **(a)**。`.github/`, `.claude/`, `.env` が見えないのは実害で、
しかも下の B-2 のとおり**ピッカーには出ているのにツリーには出ない**という不整合の
原因でもある。「見える」を既定にして、隠したい人が設定する向きが正しい。
=> aでok

### D-4. `UiColors` をどこまで細かくするか → **A-2 の粒度**

druk の `ThemeUi` は 24 ロール（`~/workspace/druk/src/themes/types.ts:4-31`）。
gargo にはタブストリップがない等、そのまま 1:1 にはならない。

- **(a) druk の 24 ロールをほぼそのまま写す** — 将来の UI 追加に耐える。
  ただし最初のプリセット 1 本を書くコストが上がる
- **(b) 今 UI が実際に使っている色だけ（10 ロール前後）から始めて、必要になったら足す**

推奨: **(b)**。ただし**派生ロールは最初から計算で出す**方針だけは (a) から借りる
（下の A-2 参照）。ロールは後から足せるが、「各テーマに border 色を手で書かせる」
設計にしてしまうと後で剥がすのが面倒。
=> bでok

### D-5. これらをどの順で入れるか

A-1（ステータスバー）は 1 箇所の修正で体感が変わるので独立して先に入れられる。
B 系は A 系と完全に独立。C 系は A-2 完了後が望ましい（テーマがないと色が決まらない）。

推奨順: **A-1 → B-1/B-2 → A-2 → A-3 → C 系**
=> これでok

---

## A. 色味・テーマ

### 現状の要約

| 項目 | 実態 |
|---|---|
| テーマの守備範囲 | tree-sitter キャプチャ → `Style{fg, bold, italic}` のみ。あと markdown ホバー用 bg 2 色（`src/syntax/theme.rs:13-17`） |
| プリセット | `ansi_dark` / `ansi_light` の 2 つだけ。中身は ANSI 16 色の**名前**（`theme.rs` 内に 108 箇所） |
| UI クローム | **テーマが存在しない**。全部ハードコード |
| ハードコード量 | `src/ui/` 配下に 72 箇所、`src/command/git.rs` に 8、`src/app.rs` に 2 |

ANSI 名前色 ＝ 実際の画面の色はユーザーのターミナル設定次第。端末を変えると別物になり、
使える色相が実質 8 個しかないので情報の描き分けができない。druk が見やすい根本理由はここ。

なお hex → `Color::Rgb` のパーサは **既にある**（`src/syntax/theme.rs:631` `parse_color`）。
config から hex で上書きする経路は通っている。足りないのは「hex でできたプリセット」と
「UI クロームのテーマ化」の 2 つだけ。

---

### A-1. ステータスバーの `reverse: true` をやめる

**規模**: 小（1 ファイル） / **依存**: なし / **単独で入れられる**

`src/ui/views/status_bar.rs:86-91` が `CellStyle { reverse: true, .. }` だけで塗っている。
reverse は端末の fg/bg をそのまま反転するので、**エディタ本体と無関係な色**になる。
ステータスバーだけ浮いて見える最大の原因。

- [x] `CellStyle` に明示的な `fg` / `bg` を指定する
- [x] 当座は `theme` 経由にできないなら定数でもよいが、A-2 実装時に必ずテーマ参照へ移す
      （TODO コメントを残す）
- [x] モード表示（`NOR` / `INS` / `VIS`）はモードごとに色を変える。druk にはない gargo 独自の
      要素だが、モーダルエディタでは現在モードが一目で分かることの価値が大きい

**完了条件**: ターミナルのカラースキームを変えてもステータスバーの見え方が変わらない。

---

### A-2. `Theme` に UI クロームのロールを追加する

**規模**: 中 / **依存**: D-1, D-4 / **A-3 の前提**

現在 `Theme`（`src/syntax/theme.rs:13`）はシンタックス色しか持たない。
UI 用の色レイヤを別に足す。

- [x] `UiColors` 構造体を定義。最低限のロール（D-4 で粒度を決める）:
      `bg` / `panel_bg` / `bar_bg` / `status_bg` / `status_fg` / `text` / `dim` / `faint` /
      `accent` / `selected_bg` / `focus_bg` / `dirty` / `error` / `folder` / `cursor` /
      `gutter` / `current_line` / `git_added` / `git_modified` / `git_deleted`
- [x] **派生ロールは列挙せず計算する**。druk が `colorsFor` でやっている方式
      （`~/workspace/druk/src/themes/types.ts:52-58` のコメントが要点）。
      罫線の色は「2 色の**関係**」であって誰かが選ぶ色ではない。
      各プリセットに `border` を手書きさせると、テーマを足すたびに微妙にズレる。
      対象: `border`（`bg` と `text` の間から導出）、`sidebar_bg`（`panel_bg` から）
- [x] `Theme` に `pub ui: UiColors` を持たせ、`Theme::from_config`（`theme.rs:539`）で
      プリセット選択とユーザー上書きを反映
- [x] `ThemeUiConfig`（`src/config.rs:198`）を拡張。現状 markdown ホバー 2 色しかない。
      既存の 2 フィールドは alias で残して後方互換を保つ
- [x] hex プリセットを最低 1 組（dark / light）追加。`parse_color` は既に hex 対応済み
- [x] `normalize_preset_name`（`theme.rs:624`）に新プリセット名を登録。
      未知の名前は既定にフォールバックする既存挙動を壊さないこと

**完了条件**: `config.toml` の `[theme] preset` で新プリセットを選ぶと、
シンタックスと UI の両方が hex の色で描画される。既存の `ansi_dark` / `ansi_light` も
引き続き動く。

---

### A-3. ハードコードされた色をテーマ参照に置き換える

**規模**: 大（機械的だが広い） / **依存**: A-2

`Color::Cyan` 等の直書きが `src/ui/` に 72 箇所。`RenderContext` には既に
`theme: &Theme` と `config: &Config` の両方がある（`src/ui/framework/component.rs:16,18`）ので、
描画側は追加の配管なしで参照できる。

多い順に潰す。1 ファイルずつ独立してコミットできる:

- [x] `src/ui/views/text_view.rs`（20 箇所）— エディタ本体。効果が最大
- [x] `src/ui/overlays/git/commit_log.rs`（12）
- [x] `src/ui/overlays/explorer/sidebar.rs`（10）— `Color::Cyan`(2000,2006),
      `Green`(2049), `Red`(2053), `DarkRed`/`DarkGreen`(2378-2379) 他
- [x] `src/ui/overlays/git/view.rs`（9）
- [x] `src/ui/overlays/github/issue_picker.rs`（7）
- [x] `src/ui/overlays/github/pr_picker.rs`（6）
- [x] `src/ui/views/notification_bar.rs`（2）、`editor/markdown_link_hover.rs`（2）、
      `editor/find_replace.rs`（2）、`project/save_as_popup.rs`（1）、
      `project/root_picker.rs`（1）
- [x] `src/command/git.rs`（8）、`src/app.rs`（2）も同様に確認

⚠️ `sidebar.rs:2767-2768` に `Color::DarkRed` / `DarkGreen` を直接アサートしている
テストがある。テーマ参照に変えると落ちるので、テスト側もテーマ経由の期待値にする。

**完了条件**: `grep -rn "Color::" src/ui | grep -v "Color::Rgb"` がテスト以外でほぼ空。

---

### A-4. テーマプリセットを増やす

**規模**: 中（1 テーマあたりは小） / **依存**: A-2, A-3

A-2/A-3 が終われば 1 テーマ ≒ 定数の塊 1 個。druk は 26 本持っている
（`~/workspace/druk/src/themes/` — tokyo-night, catppuccin ×4, gruvbox, nord,
rose-pine ×3, kanagawa ×3, everforest, ayu ×3, dracula, solarized, one-dark, vesper …）。

- [x] 参考にする場合、druk の各テーマファイルは 60 行程度で `ui` と `syntax` の
      2 ブロックに分かれている。UI ロール名を gargo 側に読み替えれば値はそのまま使える
- [x] 出典元（tokyonight.nvim 等）のライセンス表記を確認してから取り込むこと。
      druk 側は各ファイル冒頭に出典 URL のコメントを置いている

---

### A-5. OS のライト/ダーク追従（`theme_sync`）

**規模**: 中 / **依存**: A-2, A-4（明色テーマが 1 本は要る） / **優先度: 低**

druk の `src/core/appearance.ts`（115 行）が直接移植できる設計。
OS には可搬な購読 API がないのでポーリングしている。

- [x] プラットフォーム別プローブ:
      macOS `defaults read -g AppleInterfaceStyle`（キーはダーク時のみ存在＝
      読み取り失敗＝ライト）、Linux `gsettings get org.gnome.desktop.interface color-scheme`
      →`default` なら `gtk-theme` にフォールバック、Windows は `reg query`
- [x] `theme_light` / `theme_dark` を別々に設定できるようにする
- [x] 環境変数で強制上書きできる口を用意（druk の `DRUK_OS_APPEARANCE` 相当）。
      どのプローブも答えられないデスクトップ用
- [x] ポーリングスレッドが**プロセスの終了を妨げない**こと。
      druk はタイマーを unref している。Rust なら detached thread ＋ 終了フラグ

---

### A-6. パレットでのテーマライブプレビュー

**規模**: 中 / **依存**: A-4（選ぶ対象が複数ないと意味がない） / **優先度: 低**

druk はパレットの選択がその値に乗っている間だけ適用し、確定せずに抜けたら戻す
（`preview` / `restore` の 2 コールバック）。26 個から選ぶ UI として効いている。

- [x] `restore` は「**適用前の値を覚えて戻す**」のではなく「**config が言う値に戻す**」
      実装にすること。druk の設計メモに明記されている落とし穴
- [x] 対象は `src/ui/overlays/palette/`

---

## B. file tree の hidden files

### 現状の要約 — 設定がないだけでなく、**内部でルールが割れている**

| 場所 | 挙動 |
|---|---|
| `src/ui/overlays/explorer/sidebar.rs:1027-1029` | `if name.starts_with('.') { continue; }` — 無条件スキップ |
| `src/ui/overlays/explorer/popup.rs:221-224` | 同じく無条件スキップ |
| `src/project.rs:114` | ピッカーの非 git walk。`.` + `target` + `node_modules` を無条件スキップ |
| `src/command/git_backend.rs:101` `collect_files` | **git リポジトリではフィルタなし**。index エントリ + untracked をそのまま返す |
| changed-files モード `read_changed_entries` | git status 由来なのでフィルタなし |

結果として **`.github/workflows/ci.yml` は `SPC f`（ピッカー）には出るのに
`SPC e`（サイドバー）には出ない**。gargo 自身の `.claude/` も `.github/` もツリーに映らない。

### druk の設計（参考）

`~/workspace/druk/src/app/tree.ts:17-25` — 要点は 3 つ:

1. **述語を 1 個返すだけ**。`if` を各所に散らさない。ツリーを平坦化する関数がそれを受け取り、
   「隠したディレクトリには**降りない**」判断も同じ場所に集約されている
2. **既定は表示**（`show_dotfiles: true`）
3. **gitignore 尊重は独立した別設定**。「ドットファイル」と「ignore 対象」は違う概念

---

### B-1. `show_dotfiles` 設定を追加してサイドバーに適用

**規模**: 小〜中 / **依存**: D-2, D-3

- [x] `UiConfig`（`src/config.rs:96`）に `show_dotfiles: bool` を追加。既定は D-3 で決めた値
- [x] D-2 で決めた方式で `Explorer` に渡す
- [x] `sidebar.rs:1027-1029` のハードコードされたスキップを設定参照に置換
- [x] `popup.rs:221-224` も同様に置換（**同じ述語を使うこと**。ここで実装が分岐すると
      今の不整合を作り直すことになる）
- [x] テスト: ドットファイルを含む fixture で、設定 on/off 両方の `visible_entries` を検証

**完了条件**: `show_dotfiles = true` で `.github/` がサイドバーに出る。
`false` で従来どおり消える。

---

### B-2. ピッカーとツリーで「見えるファイル」を一致させる

**規模**: 中 / **依存**: B-1

B-1 だけだと、git リポジトリ内で `collect_files_git` が返す一覧との不整合が残る。

- [x] `src/project.rs:114` の walk 側のスキップを同じ設定に従わせる
- [x] `src/command/git_backend.rs:101` `collect_files` の結果にも同じ述語を適用する。
      ここは現在ノーフィルタなので、`show_dotfiles = false` のときに**初めて**
      フィルタが要るようになる（＝ B-1 の既定を `true` にした場合、この項目の
      優先度は下がる）
- [x] `target` / `node_modules` の除外は**ドットファイルとは別概念**なので、
      `show_dotfiles` と混ぜないこと。将来 B-3 の gitignore 側に寄せるのが筋

**完了条件**: 同じプロジェクトで `SPC f` に出るファイル集合と `SPC e` を辿って
到達できるファイル集合が一致する（ディレクトリ展開の差を除く）。

---

### B-3. `respect_gitignore` 設定（任意）

**規模**: 中 / **依存**: B-1 / **優先度: 低**

druk は `respect_gitignore`（既定 false）を `show_dotfiles` とは独立に持つ。
gargo は既に gix を使っているので ignore 判定のコストは低い。

- [x] `UiConfig` に `respect_gitignore: bool` を追加、既定 `false`
- [x] B-1 で作った述語に OR 条件として合流させる
- [x] ignore 集合をキャッシュする場合、**古い集合を使い続けない**こと。
      druk は「ツリー更新のたびに読み直す」設計にしていて、その理由をコメントに
      残している（ignore ルールが消えたのにファイルが隠れたままになる）

---

### B-4. サイドバー内でのトグルキー

**規模**: 小 / **依存**: B-1

- [x] エクスプローラにフォーカスがある状態で `.` を押すと `show_dotfiles` が反転して
      即座に再読み込み（設定ファイルには書かない一時トグル、が扱いとして自然）
- [x] コマンドパレットにも "Toggle Hidden Files" を登録。
      既存の "Toggle Split Diff Preview"（`UiConfig::branch_compare_split_preview`）が
      同じ形の先例なので、それに倣う

---

## C. その他 — druk から持ってこられそうなもの

いずれも A/B より優先度は低い。**着手前に本当に要るか再検討すること**。

### C-1. 「今押せるキー」の一覧表示

**規模**: 中

druk の `Ctrl+K`。ステータスバーの上に、今のペインで効くキーを一列で出し、
次の打鍵で消える。裏側は `~/workspace/druk/src/ui/keys.ts` の**単一テーブル**で、
フッターのヒント・ヘルプ画面・ピーク・ウェルカム画面が全部そこから描画される
（＝キーを足したのに 1 箇所だけ更新漏れ、が起きない）。

gargo はコマンドパレットがあるので「何ができるか」は引ける。足りないのは
「**今このペインで何が押せるか**」の面。モーダルエディタなのでモードごとに変わる分、
druk より価値が高い可能性がある。

- [x] 先に `src/input/keymap.rs` が単一の情報源になっているか確認する。
      なっていないなら、そちらの整理が本体

### C-2. 設定画面

**規模**: 大

druk はパレットから設定ページを開き、各行を `←→` で変更、`Enter` で候補一覧、
即時反映＋永続化。`config.toml` を手で開く必要がない。
gargo は現在 config.toml 手編集のみ。

A-4 でテーマが増えたときに「26 個から選ぶのに矢印を 26 回押す」問題が出るので、
A-4 とセットで検討する価値がある。

### C-3. プロジェクトローカル設定

**規模**: 中

`<project>/.gargo/config.toml` がユーザー設定を**キー単位で**上書き（VS Code 方式）。
druk は上書きされた行を `◆` で示し、Backspace で 1 行だけリセットできる。
C-2（設定画面）がないと上書き状態が見えないので、C-2 の後。

### C-4. プレビュータブ

**規模**: 中

ツリーから開いたタブは italic 表示で、次に開いたファイルに置き換わる。
編集するかダブルクリックで確定して残る。ファイルを覗くたびにバッファが増えない。

### C-5. スクロールバー横のトラック

**規模**: 中

ファイル全体の git 変更位置と LSP 診断位置を、スクロールバーの隣に別グリフの列で出す。
「ファイルのどこに変更があるか」がスクロールせずに分かる。
gargo は既に git gutter と診断を持っているので、データ源は揃っている。

---

## 参考

- druk リポジトリ: `~/workspace/druk`（Bun + TypeScript + SolidJS + OpenTUI, MIT）
- テーマのロール定義: `~/workspace/druk/src/themes/types.ts`
- テーマ実例: `~/workspace/druk/src/themes/tokyo-night.ts`
- OS 外観検出: `~/workspace/druk/src/core/appearance.ts`
- ツリーの hidden 述語: `~/workspace/druk/src/app/tree.ts:17-25`
- キー定義テーブル: `~/workspace/druk/src/ui/keys.ts`

逆に gargo にあって druk にないもの（＝この移植で失うものは何もない）:
ウィンドウ分割、マルチカーソル、マクロ / ドットリピート、jump list、
wasm のブラウザエディタ、プラグイン機構、GitHub PR/issue ピッカー、ベンチマーク一式。
