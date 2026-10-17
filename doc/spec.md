# tabex 設計書

この設計書は `doc/requirement.md` に書かれた要求を、どう実装するかを定めるものである。両者で食い違う記述がある場合は、要求仕様のほうを正とする。

## 1. 全体方針

### 1.1 基本方針

- **ファイル一覧などの部品は Windows 標準のコントロールを使い、独自の描画は最小限にとどめる。** ファイル一覧には SysListView32 の仮想モード（`LVS_OWNERDATA`）、ツリーには SysTreeView32、パス入力には Edit を使う。標準コントロールを使うことで、IME 入力・アクセシビリティ・高 DPI 対応・その場リネームを OS に任せられる。独自に描画するのは、タブバーとパンくず形式のアドレスバーだけとする。
- **フォルダの中身は自前で列挙し、エクスプローラの表示部品（IExplorerBrowser）は使わない。** 起動時間、メモリ、表示状態の保存を自分で制御するためである。通常のフォルダは `FindFirstFileExW` で高速に列挙する。ごみ箱や「PC」のような実体のない特殊フォルダだけ、シェルの `IShellFolder` で列挙する。
- **状態は UI スレッドだけが持ち、時間のかかる処理はワーカースレッドで行う。** ワーカーは結果をチャネルに送り、`PostMessageW(WM_APP_WORKER)` で UI スレッドに知らせる。UI スレッドとワーカーの間で状態をロックして共有することはしない。
- **Windows に依存しない部分を分離してテストしやすくする。** タブ操作、ワークスペースの直列化、レイアウト計算、コマンドライン解析などは `model` / `store` / `cli` モジュールにまとめ、Win32 API を呼ばない純粋なロジックとして単体テストで検証する。

### 1.2 スレッド構成

| スレッド | COM の種類 | 役割 |
|---|---|---|
| UI | STA | メッセージループ。`App` 状態の唯一の持ち主 |
| enum ×2 | STA | フォルダの列挙、ナビゲーションツリーの子項目の読み込み |
| icon ×2 | STA | アイコンとサムネイルの取得 |
| fileop ×1 | STA | `IFileOperation` を実行する（進捗ダイアログはシェルが出す） |
| watch ×1 | — | `ReadDirectoryChangesW`（overlapped I/O）でフォルダを監視する |

- ワーカーはすべて STA で初期化する。シェルの拡張機能の多くが、STA から呼ばれることを前提にしているためである。
- ワーカーへの仕事は、プールごとのキュー（`Mutex` と `Condvar`）で渡す。結果は `std::sync::mpsc` で UI スレッドに返す。
- 仕事を取り消すときは、リクエストごとに持たせた `generation: u64` を使う。UI 側は、最新の generation と一致しない結果を捨てる。

## 2. ディレクトリ構成

```
tabex/
├── Cargo.toml
├── build.rs                 # embed manifest, icon and VERSIONINFO
├── rustfmt.toml
├── .cargo/config.toml       # +crt-static
├── res/
│   ├── tabex.manifest       # comctl32 v6, PerMonitorV2, longPathAware, activeCodePage=UTF-8
│   ├── tabex.rc             # template; build.rs fills in the version
│   └── tabex.ico
├── src/
│   ├── main.rs              # entry point, panic hook, argument validation
│   ├── lib.rs
│   ├── cli.rs               # argument parsing (pure)
│   ├── instance.rs          # single instance + WM_COPYDATA forwarding
│   ├── report.rs            # messages from a GUI process (console or message box)
│   ├── model/               # platform independent state (pure, unit tested)
│   │   ├── workspace.rs     # Workspace / Pane / Tab / ViewState / Location
│   │   ├── tabs.rs          # tab open/close/move/duplicate/reopen
│   │   ├── history.rs       # back/forward
│   │   ├── listing.rs       # Entry, Listing, sorted index, filter, selection
│   │   ├── layout.rs        # window geometry
│   │   └── command.rs       # Command enum
│   ├── store/               # persistence
│   │   ├── mod.rs           # StoreError, atomic write, open_or_create
│   │   ├── paths.rs         # base dir resolution, workspace name validation
│   │   ├── workspace_file.rs# TOML DTO <-> model, version check
│   │   ├── config.rs        # config.toml
│   │   └── autosave.rs      # debounce logic
│   ├── fs/
│   │   ├── mod.rs           # FsError, long path prefix
│   │   ├── enumerate.rs     # FindFirstFileExW fast path / IShellFolder fallback, PIDL wrapper
│   │   ├── sort.rs          # comparators (CompareStringEx + SORT_DIGITSASNUMBERS)
│   │   └── watch.rs
│   ├── shell/
│   │   ├── names.rs         # input resolution, display names, known folders
│   │   ├── icons.rs         # system image lists, icon/type-name caches
│   │   ├── thumbs.rs        # thumbnail fetch and LRU cache
│   │   ├── context_menu.rs
│   │   ├── fileop.rs        # IFileOperation + progress sink
│   │   ├── undo.rs          # undo records, restore from recycle bin
│   │   ├── clipboard.rs     # data objects and clipboard formats
│   │   └── dragdrop.rs      # IDropTarget
│   ├── ui/
│   │   ├── tabbar.rs        # custom drawn (GDI, double buffered)
│   │   ├── addressbar.rs    # breadcrumb + Edit with SHAutoComplete
│   │   ├── listview.rs      # LVS_OWNERDATA virtual list helpers
│   │   ├── navtree.rs
│   │   ├── workspace_dialog.rs # workspace list popup, name prompt
│   │   ├── accel.rs         # key -> Command mapping
│   │   ├── format.rs        # size/date/attribute formatting
│   │   ├── dpi.rs
│   │   └── theme.rs         # dark title bar only
│   └── app/
│       ├── mod.rs           # App: owns state, main window procedure, layout
│       ├── commands.rs      # Command execution
│       ├── panes.rs         # binding tabs to controls, enumeration results, list notifications
│       ├── shellops.rs      # file operations, clipboard, D&D, context menu, thumbnails
│       ├── workspaces.rs    # workspace save/switch/rename/delete
│       ├── menu.rs          # menu bar and popup menus
│       ├── tree.rs          # navigation tree notifications
│       └── worker.rs        # thread pools and UI wake-up
├── tests/                   # integration tests (Windows only)
│   ├── enumerate.rs
│   ├── watch.rs
│   └── store.rs
└── examples/
    └── enum_bench.rs        # 100k entries benchmark
```

エラー型は `src/error.rs` にまとめず、各モジュールで定義する（§9）。

## 3. データモデル（`model`）

### 3.1 ワークスペース

```rust
pub struct Workspace {
    pub name: String,
    pub panes: [Pane; 2],          // [left, right]
    pub active_pane: PaneSide,
    pub layout: Layout,            // Single | Dual
    pub split_ratio: f32,          // 0.1..=0.9
}

pub struct Pane {
    pub tabs: Vec<Tab>,            // invariant: len() >= 1
    pub active_tab: usize,
}

pub struct Tab {
    pub id: TabId,                 // runtime only, not persisted
    pub location: Location,
    pub view: ViewState,
    pub history: History,          // runtime only
    pub status: TabStatus,         // runtime only
}

pub struct ViewState {
    pub mode: ViewMode,            // Details | List | IconsLarge | IconsMedium | IconsSmall
    pub sort: SortSpec,            // { column: ColumnId, order: Asc | Desc }
    pub columns: Vec<ColumnSpec>,  // { id: ColumnId, width: u32 } (width in 96-DPI units)
}

pub enum TabStatus { Loading, Ready, NotFound, Error(String) }
```

- `Location` には、シェルが解釈できる名前（parsing name）を文字列で持たせる。
  - 通常のフォルダならパスそのもの（`C:\src`、`\\server\share`）。
  - 特殊フォルダなら `shell:RecycleBinFolder` や `::{20D04FE0-3AEA-1069-A2D8-08002B30309D}`（PC）のような名前になる。
  - これらは `SHParseDisplayName` でどれも解決できるので、保存形式は文字列 1 種類で済む。
- 列幅は 96 DPI 換算の値で保存し、表示するときに DPI に合わせて拡大縮小する。
- Pane には「タブが必ず 1 つ以上ある」という不変条件を持たせる。最後のタブを閉じようとしたら、そのタブを既定フォルダ（ユーザーフォルダ）に移動させる。

### 3.2 タブ操作（`model::tabs`）

`Workspace` に対して次の関数を用意する。どれも純粋関数で、`active_tab` の付け替えまで面倒を見る。

```rust
fn open_tab(&mut self, side: PaneSide, loc: Location, at: Option<usize>, activate: bool) -> TabId;
fn close_tab(&mut self, side: PaneSide, idx: usize) -> ClosedTab;  // pushes to closed stack
fn reopen_closed(&mut self) -> Option<TabId>;                      // max 20 entries, runtime only
fn move_tab(&mut self, from: (PaneSide, usize), to: (PaneSide, usize));
fn duplicate_tab(&mut self, side: PaneSide, idx: usize) -> TabId;
fn activate(&mut self, side: PaneSide, idx: usize);
```

`History` は戻る用と進む用の 2 本のスタック（それぞれ最大 100 件）で実装する。新しい場所へ移動したら、進む用のスタックを空にする。

### 3.3 一覧データ（`model::listing`）

```rust
pub struct Entry {
    pub name: Box<[u16]>,      // UTF-16 to avoid conversion on display
    pub attrs: u32,            // FILE_ATTRIBUTE_*
    pub size: u64,
    pub modified: u64,         // FILETIME
    pub created: u64,
    pub pidl: Option<Pidl>,    // only for shell (virtual) folders
    pub icon: Cell<i32>,       // system image list index, -1 = not resolved
}

pub struct Listing {
    pub generation: u64,
    pub entries: Vec<Entry>,   // append-only during load
    pub order: Vec<u32>,       // sorted + filtered view: indices into entries
    pub complete: bool,
}
```

- 隠しファイル・システムファイルを表示しない設定は、`order` を作るときに該当項目を除くことで実現する。設定を切り替えても列挙し直す必要はない。
  - 判定には隠し属性を使う。保護された OS ファイル（隠し属性とシステム属性の両方を持つもの）も、この切り替えに従う。システム属性だけを持つファイルは、エクスプローラと同じく常に表示する。
  - ドットファイル（名前の先頭が `.`）は、`Filter::show_dotfiles` で別に判定する。`Filter::accepts` は「隠し属性の判定」と「ドットファイルの判定」の両方を通った項目だけを表示する。名前だけで決まるので、ドライブのルートを含め、追加のファイルアクセスは発生しない。
- 選択状態は `HashSet<u32>`（`entries` の添字）で別に持つ。ソート順が変わったら、ListView 側の選択状態をこの集合から付け直す。
- 再読み込み（F5、監視のあふれ、表示し直し）では新しい `Listing` を裏で作り、完了したら差し替える。選択状態とフォーカスは名前で引き継ぐ。
- `Entry::icon` の値 `-2`（`THUMB_ICON`）は「サムネイルを描く」という意味で、ListView には `I_IMAGENONE` として渡す。
- メモリを節約するため、`Listing` を持つタブは「表示中の 2 タブ」と「直前に表示していた 2 タブ」の最大 4 つに限る（LRU）。それ以外のタブは `Listing` を捨て、もう一度表示するときに列挙し直す。

### 3.4 コマンド（`model::command`）

メニュー・ショートカット・ツールバー・起動引数などからの操作は、すべて `Command` 列挙型に変換してから `App::execute(cmd)` に渡す。

```rust
pub enum Command {
    NewTab, CloseTab, ReopenClosedTab, NextTab, PrevTab, SelectTab(u8), DuplicateTab,
    SwitchPane, ToggleLayout, ToggleNavTree,
    Back, Forward, Up, Refresh, FocusAddress, Navigate(Location),
    Open, OpenInNewTab, Rename, NewFolder, Delete { permanent: bool },
    Copy, Cut, Paste, Undo, SelectAll, Properties,
    SetViewMode(ViewMode), SortBy(ColumnId), ToggleHidden, ToggleDotfiles, ToggleExtensions,
    WorkspaceList, WorkspaceNew { from_current: bool }, WorkspaceSaveAs(String),
    WorkspaceRename(String), WorkspaceDelete(String), WorkspaceSwitch(String),
}
```

## 4. 永続化（`store`）

### 4.1 保存場所

- 保存先の基準ディレクトリは `%APPDATA%\tabex` とし、`SHGetKnownFolderPath(FOLDERID_RoamingAppData)` で取得する。
- 環境変数 `TABEX_HOME` が設定されていれば、そちらを基準ディレクトリにする。これは開発・テスト用であり、README では開発者向けとして扱う。
- 構成:
  - `config.toml`
  - `workspaces\<name>.toml`

### 4.2 ワークスペース名の制約（`store::paths::validate_name`）

ワークスペース名はそのままファイル名に使うため、名前を加工せず、次の条件を満たさない名前は受け付けない（ダイアログでエラー表示する）。

- 1〜64 文字
- 次の文字を含まない: `< > : " / \ | ? *` と制御文字
- 末尾が `.` や空白でない
- 予約名（`CON`、`NUL`、`COM1` など）でない
- 既存のワークスペース名と大文字・小文字の違いだけで重複しない

### 4.3 ファイル形式

- `serde` を使い、TOML ファイルと 1 対 1 に対応する DTO を介して読み書きする。形式は要求仕様 `使いかた` の例に従う。
- DTO はモデルとは別の型にし、`From` / `TryFrom` で相互に変換する。こうしておくと、ファイル形式を変えてもモデルに影響しない。
- `version = 1` を必須とする。
  - 知らない（より新しい）version のファイルは読み込みエラーにし、上書きもしない。
- 項目が欠けていても、可能な限り既定値で補う。
  - `view` が無ければ詳細表示、`sort` が無ければ名前の昇順、`columns` が無ければ標準の 4 列とする。
  - タブが 0 件のペインには、既定フォルダのタブを 1 つ入れる。
- `config.toml` の形式:

```toml
version = 1
last_workspace = "dev"
recent_workspaces = ["dev", "photo"]   # max 10
show_hidden = false
show_dotfiles = false                  # dot files are hidden by default
show_extensions = true
nav_tree_visible = true
window = { x = 100, y = 100, width = 1400, height = 900, maximized = false }

[keys]                                  # optional overrides, "Ctrl+Alt+W" = "WorkspaceList"
```

### 4.4 自動保存（`store::autosave`）

- ワークスペースに変更が入るたびに dirty フラグを立て、最後の変更から 1 秒たったら保存する（`SetTimer` で実装する）。
- 1 秒を待たずにすぐ保存する場面: ワークスペースを切り替えるとき、ウィンドウを閉じるとき（`WM_CLOSE`）、Windows のログオフ・シャットダウン時（`WM_ENDSESSION`）。
- 書き込みは途中で壊れないように行う。いったん `<name>.toml.tmp` に書き出してから、`MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` で本来のファイル名に置き換える。
- どの変更でタイマーを始め、いつ保存するかの判定ロジックは `Debouncer { last_change: Option<Instant> }` として切り出す。時刻は引数で渡し、テストで差し替えられるようにする。
- 読み込みに失敗したワークスペースは「保存禁止」の印を付け、壊れたファイルを自動保存で上書きしないようにする。

### 4.5 起動時・切替時の流れ

1. `config.toml` を読む。読めない場合は既定値で続ける。
2. 開くワークスペースを決める。`--workspace` の指定があればそれ、なければ `last_workspace` とする。
3. そのワークスペースファイルを読む。
   - ファイルが無い場合（初回起動）: `default` ワークスペースを作る。左ペインはユーザーフォルダ、右ペインはダウンロードフォルダとする。
   - ファイルが壊れている場合: エラーを通知し、まだ使われていない名前（`untitled`、`untitled-2`、…）で新しいワークスペースを作る。壊れたファイルには手を触れない。
4. 左右ペインのアクティブなタブだけ列挙を始める。その他のタブはアクティブになった時点で列挙する。
5. 開いたワークスペースを先頭にして `recent_workspaces` を更新する。

- アクティブなワークスペースを削除した場合: `recent_workspaces` の中で次に新しいワークスペースに切り替える。ほかに 1 つもなければ `default` を作る。

## 5. ファイルシステム（`fs`）

### 5.1 列挙（`fs::enumerate`）

```rust
pub fn enumerate(req: EnumRequest, sink: &dyn Fn(EnumEvent)) ;
pub enum EnumEvent { Chunk(Vec<Entry>), Done, Failed(FsError) }
```

- **通常のフォルダ（高速経路）:** `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` で列挙する。8.3 形式の短い名前は取得しない。
  - パスが 260 文字を超える場合は、`\\?\` を付けてから API に渡す。UNC パスの場合は `\\?\UNC\` を付ける。
- **特殊フォルダ:** `SHParseDisplayName` で PIDL を得て、`IShellFolder::EnumObjects` で列挙する。属性やサイズは `IShellFolder2::GetDetailsEx` で取得する。
- **小分けにして送る:** 最初の `Chunk` は 1,000 件たまった時点ですぐ送る。2 つ目以降は 50ms ごとにまとめて送る。件数で細かく区切ると、UI 側のマージ（1 回につき O(n)）が積み重なって遅くなるためである。10 万件のとき、件数で区切ると UI 側のマージに合計 350ms かかったが、この方式では 10ms で済んだ。
  - 送る前に、各 `Chunk` をワーカー側で現在のソート順に並べておく。
  - UI 側は受け取った `Chunk` を `entries` の末尾に追加し、ソート済みの `order` に O(n) でマージする。
  - これで、10 万件のフォルダでも読み込みを待たずに、最初の行がすぐ表示される。
- **取り消し:** 列挙中に `generation` が古くなったら（別のフォルダへ移動した、タブを閉じたなど）、列挙ループを途中で抜ける。
- **フォルダが見つからない場合:** `ERROR_PATH_NOT_FOUND` / `ERROR_FILE_NOT_FOUND` / `ERROR_BAD_NETPATH` は `FsError::NotFound` に変換し、タブの状態を `TabStatus::NotFound` にする。

### 5.2 ソート（`fs::sort`）

- 名前の比較には `CompareStringEx(NORM_IGNORECASE | SORT_DIGITSASNUMBERS)` を使う。エクスプローラの `StrCmpLogicalW` と同じ自然順になり、しかも文字列の末尾に NUL がなくても比較できる。大文字と小文字は区別しない。
- 種類の列と拡張子の列は、どちらも拡張子の並びでソートする。種類名を取得するにはシェルを呼ぶ必要があるため、ソート時には使わない。
- フォルダは常にファイルより前に並べる。これはエクスプローラと同じ動きで、昇順・降順を切り替えても変わらない。
- 名前以外の列（サイズなど）でソートしたとき、値が同じ項目は名前の昇順で並べる。
- 並べ替えは UI スレッドで行う。10 万件を計測した結果、すでに並んでいる状態からなら 8〜12ms、並び順がばらばらな最悪のケースでも約 120ms だった。ソート列を変えたときだけの処理なので、許容範囲と判断した。もっと速さが必要になったら、ワーカー側で行う方式に変える。

### 5.3 フォルダの監視（`fs::watch`）

- 監視するのは表示中のタブ（最大 2 つ）のフォルダだけとする。
  - 表示していないタブは監視せず、`Listing` に「古い可能性あり」の印を付ける。そのタブをもう一度表示するときに列挙し直す。
- 監視には `ReadDirectoryChangesW` を overlapped I/O で使い、監視スレッドで `WaitForMultipleObjects` を回す。
- 通知はすぐには反映せず、100ms ためてからまとめて UI に送る。
  - UI は受け取った追加・削除・名前変更・更新を、`entries` と `order` に差分として反映する（挿入位置は二分探索で求める）。
  - 通知用のバッファがあふれたら（`ERROR_NOTIFY_ENUM_DIR`）、差分では追いつけないので、フォルダ全体を列挙し直す。
- ネットワークフォルダの監視に失敗した場合は監視をあきらめる。そのタブで F5 を押したときに列挙し直す。

## 6. シェル統合（`shell`）

### 6.1 アイコンとサムネイル（`shell::icons`）

- アイコンはシステムイメージリスト（`SHGetImageList`）を ListView に設定して表示する。
- 一覧を表示した直後は `SHGetFileInfoW(SHGFI_USEFILEATTRIBUTES | SHGFI_SYSICONINDEX)` を使い、拡張子だけから決まるアイコンを付ける。ディスクにアクセスしないので速い。
- `.exe`、`.ico`、`.lnk`、`.url` と、独自のアイコンを持つフォルダは、icon ワーカーで本来のアイコンを取得し、取れたらその行だけを描き直す。対象は画面に見えている行だけとする。
- アイコン表示の大きさは、大 = 256px（`SHIL_JUMBO`）、中 = 48px（`SHIL_EXTRALARGE`）、小 = 16px とする。256px と 48px のイメージリストはメモリを多く使うので、その表示モードを初めて使うときに取得する。
- サムネイルを表示するのはアイコン表示（大・中）のときだけとし、`IShellItemImageFactory::GetImage(SIIGBF_THUMBNAILONLY)` で取得する。
  - 取得したサムネイルはメモリ上の LRU キャッシュ（上限 32MB、HBITMAP）に保存する。サムネイルを持たないファイルも覚えておき、何度も問い合わせない。
  - サムネイルは ListView のカスタム描画（`NM_CUSTOMDRAW` の `CDDS_ITEMPOSTPAINT`）で、アイコンの領域に描く。
- 種類の列に表示する名前は `SHGetFileInfoW(SHGFI_TYPENAME)` で取得し、拡張子ごとにキャッシュする。

### 6.2 右クリックメニュー（`shell::context_menu`）

- 項目を右クリックしたとき: `IShellFolder::GetUIObjectOf(IID_IContextMenu)` でメニューを作る。
- 何もない所を右クリックしたとき: `IShellFolder::CreateViewObject(IID_IContextMenu)` で背景用のメニューを作る。
- `IContextMenu3::HandleMenuMsg2` に、`WM_INITMENUPOPUP`、`WM_DRAWITEM`、`WM_MEASUREITEM`、`WM_MENUCHAR` を転送する。これをしないと、他のアプリが追加したメニュー項目（サブメニューやアイコン付きの項目）が正しく表示されない。
- 選ばれたメニュー項目の動詞（verb）が `rename`、`delete`、`copy`、`cut`、`paste`、`newfolder` の場合はシェルに処理させず、tabex の `Command` として実行する。これは、元に戻す操作を tabex 自身の履歴で管理し、その場リネームを使うためである。

### 6.3 ファイル操作（`shell::fileop`, `shell::undo`）

- ファイル操作は fileop ワーカーで `IFileOperation` を使って実行する。
  - オーナーウィンドウにはメインウィンドウを指定する。
  - フラグは `FOF_ALLOWUNDO | FOFX_ADDUNDORECORD | FOFX_RECYCLEONDELETE` とする（Shift+Delete で完全に削除するときは、ごみ箱関係のフラグを外す）。
  - 上書きの確認と進み具合の表示はシェルが行う。
- `IFileOperationProgressSink` で、各項目が実際にどこへどう処理されたかを受け取り、`UndoRecord` として記録する。
- 元に戻す操作（Ctrl+Z）は、tabex が自前で持つ履歴（最大 50 件）を使い、記録しておいた操作の逆を実行する。

| 操作 | 逆操作 |
|---|---|
| コピー | コピーしてできた項目をごみ箱へ移す |
| 移動 | 元の場所へ移動し直す |
| 名前の変更 | 元の名前に戻す |
| 新しいフォルダの作成 | 中身が空であれば、ごみ箱へ移す |
| ごみ箱への削除 | ごみ箱から元の場所に戻す（`IFileOperation` でごみ箱内の項目を元のパスへ移動する） |

- 完全な削除（Shift+Delete）は元に戻せない。この操作をしても、それより前の操作の履歴はそのまま残す。

### 6.4 クリップボード（`shell::clipboard`）

- コピー / 切り取り:
  - 選択している項目を `SHCreateDataObject` でデータオブジェクトにまとめ、`OleSetClipboard` に渡す。
  - 切り取りのときは、データに `Preferred DropEffect = DROPEFFECT_MOVE` を付ける。
- 貼り付け:
  - `OleGetClipboard` でデータを取り出し、`IFileOperation::CopyItems` または `MoveItems` で処理する。
  - 移動のときは、処理が終わったら `Performed DropEffect` と `Paste Succeeded` を設定する。これでエクスプローラとの間で切り取り・貼り付けをしても正しく動く。

### 6.5 ドラッグ＆ドロップ（`shell::dragdrop`）

- **ドロップを受ける側:** windows-rs の `#[implement(IDropTarget)]` で実装し、ListView・タブバー・ナビゲーションツリーに登録する。
  - ファイルをドラッグしたままタブの上で 500ms 止まると、そのタブに切り替える。タブにそのままドロップした場合は、そのタブが開いているフォルダが送り先になる。
  - ドロップの効果はエクスプローラと同じ規則で決める。同じドライブなら移動、別のドライブならコピー。Ctrl を押していればコピー、Shift を押していれば移動とする。
  - ドラッグ中の見た目は `IDropTargetHelper` で表示する。
- **ドラッグを始める側:** ListView の `LVN_BEGINDRAG` を受けて `SHDoDragDrop` を呼ぶ。
- **タブの並べ替え:** タブバーのドラッグはマウスキャプチャで処理し、OLE のドラッグ＆ドロップは使わない。ドロップした位置に応じて `Command::MoveTab` を発行する。反対側のペインのタブバーに落とせば、タブがそちらへ移る。

## 7. UI（`ui`）

### 7.1 ウィンドウ構成

```
┌──────────────────────────────────────────────────────────┐
│ メニューバー（ファイル / 編集 / 表示 / ワークスペース）       │
├──────────┬──────────────────────┬────────────────────────┤
│          │ [tab][tab][+]        │ [tab][+]               │  ← tabbar (custom)
│ navtree  │ ← → ↑ C:\ › src ›    │ ← → ↑ D:\ › data       │  ← addressbar
│ (TreeV.) │ ListView (ownerdata) │ ListView (ownerdata)   │
│          │                      │                        │
├──────────┴──────────────────────┴────────────────────────┤
│ status bar: 123 項目 / 3 個選択 / 4.2 MB                 │
└──────────────────────────────────────────────────────────┘
```

- 各部品の配置は `model::layout::compute(client: Rect, dpi: u32, state: &LayoutState) -> LayoutRects` で計算する。これは純粋関数なので、テストで検証できる。ウィンドウのサイズが変わったら、`DeferWindowPos` でまとめて配置し直す。
- アクティブなペインは、タブバーの下に 2px のアクセントカラーの線を引いて示す。
- コモンコントロールは v6 を使う（マニフェストで指定する）。ListView には `SetWindowTheme(L"Explorer")` を適用し、見た目をエクスプローラに合わせる。

### 7.2 ファイル一覧（`ui::listview`）

- `LVS_OWNERDATA` を指定して仮想モードで使う。`LVN_GETDISPINFO` が来たら `order[i]` から `Entry` を引いて文字列とアイコンを返す。画面に見えている行の分しか問い合わせが来ないので、10 万件あっても描画は軽い。
- 表示モードは `LV_VIEW_DETAILS` / `LV_VIEW_LIST` / `LV_VIEW_ICON` で切り替え、アイコンの大きさはイメージリストを替えて変える。
- 列見出しをクリックしたら `Command::SortBy` を発行する。列の幅や順番が変わったら `ViewState` を更新し、自動保存の dirty フラグを立てる。
- その場リネームは `LVM_EDITLABEL` で始める。これは標準の Edit コントロールなので、IME もそのまま使える。
  - 編集を始めたら、拡張子の手前までを選択した状態にする。
- 列の種類: 名前・更新日時・作成日時・種類・サイズ・属性・拡張子。標準で表示するのは名前・更新日時・種類・サイズの 4 列。

### 7.3 タブバー（`ui::tabbar`）

- 独自のウィンドウクラスとして作り、GDI でダブルバッファリングして描画する。
- タブの幅は、最小 80px・最大 220px の間で、ペインの幅に合わせて均等に縮める。全部が収まらなくなったら、左右にスクロールボタンを出す。
- マウス操作:
  - 左クリック: そのタブに切り替える
  - 中クリック: タブを閉じる
  - ダブルクリック: タブを複製する
  - ドラッグ: タブを並べ替える
- 右クリックでタブ用のメニューを出す（閉じる / 他を閉じる / 複製 / 反対側のペインへ移動）。
- タブバーの上でマウスホイールを回すと、タブを切り替える。

### 7.4 アドレスバー（`ui::addressbar`）

- 普段はパンくずを独自に描画する。区切りの `›` をクリックすると、そのフォルダの中にあるサブフォルダの一覧をドロップダウンで出す。
- 空いている所をクリックするか Ctrl+L を押すと、パンくずの代わりに Edit コントロールを出してパスを直接入力できる。
  - 入力補完は `SHAutoComplete(SHACF_FILESYS_DIRS)` で付ける。
  - Enter を押すと `SHParseDisplayName` でパスを解決して移動する。
  - 解決できない場合は Edit の枠を赤くし、ステータスバーにエラーを表示する。

### 7.5 ナビゲーションツリー（`ui::navtree`）

- 最上位の項目は、クイックアクセス・各ドライブ・ネットワーク・ごみ箱とする。
- 子の項目は、ノードを展開したときに enum ワーカーで列挙して追加する（`TVN_ITEMEXPANDING` を受けて非同期で処理する）。
- ツリー全体を作るのは、ウィンドウを表示したあと最初のアイドル時とし、起動時間を延ばさないようにする。
- 子の項目を追加するときに、隠しファイルとドットファイルの表示設定で絞り込む。どちらかの設定を切り替えたら、ツリーを最上位から作り直す（展開済みの子は読み込み直す）。

### 7.6 ワークスペース一覧（`ui::workspace_dialog`）

- Ctrl+Alt+W で開く、モードレスの小さなポップアップとする。
  - 上部に名前の絞り込み用の Edit を置き、その下に最近使った順で一覧を並べる。
  - Enter で選んだワークスペースに切り替え、Esc で閉じる。
- 一覧の項目を右クリックすると、名前の変更と削除ができる。削除するときは確認ダイアログを出す。
- メニューの「ワークスペース」には、新規 / 名前を付けて保存 / 名前の変更 / 削除と、最近使ったワークスペース（最大 10 件）を並べる。

### 7.7 キーボード（`ui::accel`）

- キーと `Command` の対応表を持つ。既定の表は下記のとおりで、`config.toml` の `[keys]` に書いた内容で上書きできる。
- 割り当てのない Alt+英字キーは、`WM_SYSCOMMAND(SC_KEYMENU)` としてメインウィンドウに送る。こうすると、フォーカスがどのコントロールにあってもメニューバーが開く。
- キー入力はメッセージループの `TranslateAcceleratorW` より前で判定する。ただし Edit コントロール（リネーム中やアドレス入力中）にフォーカスがあるときは、文字の入力・編集に使うキー（Backspace など）を横取りしない。

| キー | Command | キー | Command |
|---|---|---|---|
| Ctrl+T | NewTab | Ctrl+W | CloseTab |
| Ctrl+Shift+T | ReopenClosedTab | Ctrl+Tab / Ctrl+Shift+Tab | NextTab / PrevTab |
| Ctrl+1..9 | SelectTab(n) | Tab | SwitchPane |
| Alt+← / Alt+→ / Alt+↑ | Back / Forward / Up | Backspace | Up |
| F5 | Refresh | Ctrl+L / Alt+D | FocusAddress |
| Enter | Open | Ctrl+Enter | OpenInNewTab |
| F2 | Rename | Ctrl+Shift+N | NewFolder |
| Delete / Shift+Delete | Delete | Ctrl+C / Ctrl+X / Ctrl+V | Copy / Cut / Paste |
| Ctrl+Z | Undo | Ctrl+A | SelectAll |
| Alt+Enter | Properties | Ctrl+H | ToggleHidden |
| Ctrl+Shift+H | ToggleDotfiles | | |
| Ctrl+Alt+W | WorkspaceList | Ctrl+Shift+1..5 | SetViewMode |

### 7.8 DPI とテーマ

- DPI はモニターごとの設定に対応する（Per-Monitor V2）。`WM_DPICHANGED` を受けたら、フォントとイメージリストを作り直し、レイアウトを計算し直す。
- OS がダークモードのときは、`DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)` でタイトルバーだけを暗くする。ウィンドウの中身までは対応しない（要求仕様で対象外としたため）。

## 8. 起動、単一インスタンス、コマンドライン

### 8.1 コマンドライン（`cli`）

```rust
pub enum CliAction {
    Run { workspace: Option<String>, paths: Vec<String> },
    Help,
    Version,
}
pub fn parse(args: impl Iterator<Item = OsString>) -> Result<CliAction, CliError>;
```

外部のクレートは使わず、自前で解析する。

| 引数 | 説明 |
|---|---|
| `<path>...` | アクティブペインにタブを追加し、指定のフォルダを開く。複数指定できる |
| `-w, --workspace <name>` | 指定のワークスペースを開く。起動中であれば、そのワークスペースに切り替える |
| `-h, --help` | 使いかたを表示する |
| `-V, --version` | `tabex <version>` を表示する |

| 終了コード | 意味 |
|---|---|
| 0 | 正常終了（`--help` / `--version` の表示、起動中のインスタンスへの引数転送を含む） |
| 1 | 初期化に失敗した（ウィンドウを作れない、COM を初期化できないなど） |
| 2 | 引数が不正、または指定したフォルダ・ワークスペースが存在しない |

- tabex は GUI アプリ（`windows_subsystem = "windows"`）なので、標準出力に書いても通常は表示されない。そこで、メッセージを出すときはまず `AttachConsole(ATTACH_PARENT_PROCESS)` を試す。成功すれば起動元のコンソールに表示し、失敗した場合（エクスプローラからダブルクリックで起動したときなど）は `MessageBoxW` で表示する。
- 指定されたフォルダとワークスペースが存在するかどうかは、起動中のインスタンスに引数を転送する前に、新しく起動したプロセスの側で確かめる。

### 8.2 単一インスタンス（`instance`）

1. 名前付き mutex `Local\tabex-single-instance-<基準ディレクトリのハッシュ>` を作る。`Local\` 名前空間はログオンセッション（≒ユーザー）ごとに分かれる。基準ディレクトリをキーにするのは、`TABEX_HOME` を指定したテスト用インスタンスを、普段使いの tabex と同時に起動できるようにするためである。
2. すでに存在していれば、tabex が起動中である。クラス名が `TabexMainWindow` で、かつウィンドウプロパティ `TabexInstanceKey` が同じキーを持つウィンドウを探し、引数を `WM_COPYDATA` で送る。中身は UTF-8 の TOML とする（`RunArgs` を直列化したもの）。
3. `AllowSetForegroundWindow` を呼んで起動中のウィンドウが前面に出られるようにしてから、終了コード 0 で終了する。
4. 引数を受け取った側は、それを `Command`（`WorkspaceSwitch` → 各パスの `Navigate` in new tab）に変換して実行し、自分のウィンドウを前面に出す。

### 8.3 起動を速くするための工夫（目標 300ms）

- 起動処理の順番: 引数の解析 → 単一インスタンスの確認 → `OleInitialize` → 設定とワークスペースの読み込み → ウィンドウの作成 → `ShowWindow` → アクティブな 2 タブの列挙開始
- ナビゲーションツリー、アイコンの本取得、アクティブでないタブの準備は、ウィンドウを表示したあと最初のアイドル時まで遅らせる。
- 起動時間の計測: 環境変数 `TABEX_TRACE=1` を設定すると、起動の各段階にかかった時間を `%LOCALAPPDATA%\tabex\trace.log` に書き出す。あわせて、実行したコマンドも記録する。
- 計測結果（S11）: 起動から操作可能になるまで約 110〜130ms（ディスクキャッシュが温まっている状態）。ビルド直後の初回起動では約 350ms だった。待機時のワーキングセットは、10 万件のフォルダを表示した状態で 45.6MB。exe のサイズは 1.0MB。

## 9. エラーハンドリング

- モジュールごとに `thiserror` でエラー型を定義する。Win32 API や COM が返すエラー（`windows::core::Error`）は、各モジュールの境界で変換する。

| エラー型 | 主な発生元 | 利用者への見せ方 |
|---|---|---|
| `CliError` | 引数の解析・検証 | コンソールまたはメッセージボックスに表示し、終了コード 2 で終わる |
| `StoreError::{Io, Parse, UnsupportedVersion, InvalidName}` | 設定やワークスペースの読み書き | 読み込みの失敗はメッセージボックスで知らせ、起動は続ける。書き込みの失敗はステータスバーに表示し、次の自動保存で再試行する。名前が不正な場合はダイアログ内に表示する |
| `FsError::{NotFound, AccessDenied, Other}` | フォルダの列挙・監視 | タブの中に表示し、「再試行」と「親フォルダへ」のボタンを出す |
| `ShellError` | 右クリックメニュー・クリップボード・D&D | ステータスバーに表示する |
| ファイル操作の失敗 | `IFileOperation` | エラー表示はシェルのダイアログに任せ、tabex 側では表示しない |

- パニックしたときは `panic::set_hook` で `%LOCALAPPDATA%\tabex\crash.log` にメッセージとバックトレースを書き出し、メッセージボックスで知らせてから終了する。リリースビルドでは `panic = "abort"` とする。
- 終了時に dirty なワークスペースがあれば保存するが、パニック時には保存しない。壊れた状態を書き込むおそれがあるため。

## 10. パッケージング

- `Cargo.toml` の要点:

```toml
[package]
name = "tabex"
version = "0.1.0"
edition = "2024"

[dependencies]
windows = { version = "0.62", features = [ ...Win32 features... ] }
windows-core = "0.62"   # for #[implement] (IDropTarget, IFileOperationProgressSink)
serde = { version = "1", features = ["derive"] }
toml = "1"
thiserror = "2"

[build-dependencies]
embed-resource = "3"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

- 依存するクレートは、上記の 5 つ（`windows`、`windows-core`、`serde`、`toml`、`thiserror`）とビルド用の `embed-resource` に限る。新しく追加する場合は、起動時間と exe サイズへの影響を確かめてからにする。
- ランタイム DLL がなくても動く単一 exe にするため、`.cargo/config.toml` で `x86_64-pc-windows-msvc` に `-C target-feature=+crt-static` を指定する。
- バージョン番号は `Cargo.toml` だけで管理する。
  - `--version` の表示には `env!("CARGO_PKG_VERSION")` を使う。
  - exe に埋め込む `VERSIONINFO` には、`build.rs` が同じ値を `.rc` に流し込む。
- マニフェストで次の 4 つを宣言する: comctl32 v6、`PerMonitorV2`、`longPathAware`、`activeCodePage=UTF-8`。
- exe サイズの目安は 3MB 以下とする。

## 11. テスト方針

### 11.1 単体テスト（各モジュール内の `#[cfg(test)]`）

| 対象 | 主な観点 |
|---|---|
| `model::tabs` | タブを開く・閉じる・移動・複製したとき、`active_tab` が正しく付け替わること。最後の 1 タブを閉じたときの動き。ペインをまたぐ移動。閉じたタブの再オープン（上限の 20 件を超えたとき） |
| `model::history` | 戻る・進むの動き。新しく移動したら進む用の履歴が消えること。上限の 100 件 |
| `model::listing` | チャンクをマージした結果が、全件を一度にソートした結果と一致すること。差分の挿入・削除。隠しファイルとドットファイルの絞り込み（片方だけ、両方）。ソート順を変えたあとの選択状態の付け直し |
| `model::layout` | 1 ペイン / 2 ペインの配置。`split_ratio` の範囲外の値が丸められること。ナビゲーションツリーの表示 / 非表示。DPI 150% のとき |
| `store::workspace_file` | 要求仕様にある TOML の例が読めること。読み込みと保存を往復しても内容が変わらないこと。項目が欠けているときの既定値。知らない version は読み込みエラーになること。構文エラーのとき |
| `store::paths::validate_name` | 禁止文字・予約名・末尾が `.` や空白・長さ 0 と 65 文字・大文字小文字だけ違う名前の重複 |
| `store::autosave` | 時刻を差し替えて、1 秒間変更がなかったときだけ保存すること。すぐ保存すべき場面（切替時など）で保存すること |
| `cli` | 各オプション・複数のパス・不明なオプション・`--workspace` の値の抜け・`--help` と他の引数の混在 |
| `fs::sort` | 自然順（`file2` が `file10` より前）、フォルダが先に来ること、昇順・降順、値が同じときの並び |
| `model::workspace` | 親フォルダの求め方（ドライブのルート、UNC）、パンくずの分割、タブタイトル |
| `ui::accel` | キー指定の解析、既定の割り当て、`config.toml` による上書きと不正な指定の警告、編集中に横取りしないキー |
| `ui::format` | サイズ（KB 表記、人が読みやすい表記）、属性の表記 |
| `shell::fileop` | 元に戻す操作の逆操作（名前の変更、移動、ドライブ直下への移動） |
| `shell::dragdrop` | ドロップ効果の決定（Ctrl・Shift、同じドライブかどうか、UNC、許可されていない効果） |

### 11.2 結合テスト（`tests/`、Windows 上で実行する）

- `enumerate.rs`: 一時ディレクトリに次のようなファイルを作って列挙し、正しく取得できることを確かめる。
  - 日本語名のファイル、隠しファイル、260 文字を超えるパスのファイル
  - 1 万件のファイル（チャンクに分けて届くこと、`Done` が来ること）
  - 存在しないパスを渡したら `NotFound` になること
- `watch.rs`: 監視中のフォルダでファイルを作成・名前変更・削除し、2 秒以内に対応する通知が届くことを確かめる。
- `store.rs`: `TABEX_HOME` を一時ディレクトリに向けて、次を確かめる。
  - 保存した結果、`.tmp` ファイルが残らないこと
  - 壊れたファイルを上書きしないこと
  - 初回起動で `default` が作られること

### 11.3 性能測定

- `cargo run --release --example enum_bench -- <dir> --create 100000` で、10 万件の列挙にかかる時間を計測する。最初のチャンクが届くまでと、全件がそろうまでの 2 つの時間を出す。
- 起動時間は `TABEX_TRACE=1` を設定して起動し、トレースログで計測する。
- メモリ使用量はタスクマネージャー、または `GetProcessMemoryInfo` で確かめる。

### 11.4 手動確認

GUI の見た目と操作、シェル統合（右クリックメニュー・D&D・エクスプローラとのクリップボードのやり取り）は自動テストの対象にしない。`doc/plan.md` に確認手順を書き、手で確かめる。
