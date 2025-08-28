此 crate 可作为 Rust 库或 C 动态库使用。

## 用法

### 作为 Rust 库

将此 crate 添加为依赖项并调用 `register_offset_vfs` 即可，vfs的名称为常量`OFFSET_VFS_NAME`。
```sh
cargo add sqlite_ext_ntqq_db
```

### 作为 SQLite Runtime Loadable Extension

1. 自行编译或下载预编译得到的对应平台的动态链接库文件（由于此 package 很少有变动，因此 ci 不会发布，并非每次 release 都有动态链接库构建，[目前的最新发布在此下载](https://github.com/artiga033/ntdb_unwrap/releases/tag/v0.1.0)）

2. 加载并使用扩展，参考下方[示例](#示例)，已验证 sqlcipher 命令行工具和 DB Browser for SQLCipher(Windows) 上均可正常工作，其它 SQLite 操作程序请自行研究如何加载扩展。

#### 示例

##### sqlcipher 命令行
**step by step**:

```sh
$ sqlcipher
SQLite version 3.46.1 2024-08-13 09:16:08 (SQLCipher 4.6.1 community)
Enter ".help" for usage hints.
Connected to a transient in-memory database.
Use ".open FILENAME" to reopen on a persistent database.
sqlite> .load libsqlite_ext_ntqq_db.so
sqlite> .open nt_msg.db
sqlite> pragma key = 'YOUR_DB_KEY'; pragma kdf_iter = 4000; pragma cipher_hmac_algorithm = HMAC_SHA1;
ok
sqlite> SELECT * FROM group_msg_table LIMIT 10;
```

**one-liner**:

```sh
$ sqlcipher -cmd ".load libsqlite_ext_ntqq_db.so" -cmd ".open nt_msg.db" -cmd "pragma key = 'YOUR_DB_KEY'; pragma kdf_iter = 4000; pragma cipher_hmac_algorithm = HMAC_SHA1;"
ok
SQLite version 3.46.1 2024-08-13 09:16:08 (SQLCipher 4.6.1 community)
Enter ".help" for usage hints.
sqlite> SELECT * FROM group_msg_table LIMIT 10;
```

##### DB Browser for SQLite （Windows）
<details>
<summary> 点击展开 DB Browser for SQLite (🪟Windows) 作法</summary>

1. 从[release](https://github.com/artiga033/ntdb_unwrap/releases/tag/v0.1.0)下载[`sqlite_ext_ntqq_db-x86_64-pc-windows-msvc.zip`](https://github.com/artiga033/ntdb_unwrap/releases/download/v0.1.0/sqlite_ext_ntqq_db-x86_64-pc-windows-msvc.zip)，解压得到里面的dll文件，注意**不要修改dll的文件名**。

2. 打开 DB Browser for SQLite，选择`文件`->`新建内存数据库`，之后的弹窗直接关掉，然后点击`工具`->`加载扩展`，选择第1步中的dll文件。
![Image](https://github.com/user-attachments/assets/8eaa1d2b-db1f-40bb-8473-f03b64862416)

3. 提示“扩展已成功加载”，此时再`打开数据库`，直接选择 NTQQ 的.db文件，然后正常输入密码、加密方式即可直接打开数据库。
![Image](https://github.com/user-attachments/assets/6fd3ea8f-049f-4dd8-9fb5-53a3de749662)

**PS**：如果不想每次都手动选择加载扩展，也可以到`编辑`->`首选项`->`扩展` 中，在“选择每个数据库要加载的扩展”一栏添加该DLL。不过要注意的是，即便这样也要**先新建内存数据库**，再去打开文件，因为 sqlbrowser 的设计是在连接建立后才加载扩展，因此必须先建一个memdb来触发扩展加载。

**PS2**：不建议处理正常数据库时也开着这个扩展，虽然会尽可能fallback到默认VFS，不过由于用于探测是否是QQ数据库的方法很原始，存在一定出错的可能。
</details>

#### 注意事项
- 此扩展的设计目标为打开NTQQ数据库，即检测到NTQQ数据库文件特征时自动偏移1024个字节，其他情况会 fallback 到默认vfs。
这一行为通常不会有问题，不过**仍然建议您在操作正常数据库时不要加载此扩展**。

- 由于 SQLite 扩展的[函数入口点名称与库文件名强相关](https://www.sqlite.org/loadext.html#:~:text=If%20your%20shared%20library%20ends%20up%20being%20named%20%22YourCode.so%22%20or%20%22YourCode.dll%22%20or%20%22YourCode.dylib%22%20as%20shown%20in%20the%20compiler%20examples%20above%2C%20then%20the%20correct%20entry%20point%20name%20would%20be%20%22sqlite3_yourcode_init%22.)，因此请不要修改文件名。

## 构建

### 构建 C 动态链接库

由于此 crate 同时作为 Rust 库，而上游 `libsqlite3-sys` 的一些 feature 与 `loadable_extension` 存在冲突。
因此本 crate 也采用 feature 区分，默认情况下，作为 Rust crate，无法构建为 SQLite Runtime Loadable Extension。

要构建为 SQLite 扩展，**必须启用 `_cdylib` feature**，虽然在关闭 `_cdylib` 特性时也会生成 .so/.dll 产物，但由于没有 sqlite extension 入口函数，因此实际无法使用。该特性默认未启用，这是为了保证 Rust crate 的下游依赖的流畅体验。

请注意，不能使用 `--workspace` 和 `--features _cdylib` 一起构建，因为workspace crate 有一个与 `libsqlite3-sys` 的 `loadable_extension` 不兼容的 `sqlcipher` 特性。
因此 workspace 构建只能成功二者其一，要么是得到一个不可用的扩展库，要么是由于 feature 冲突 workspace crate 编译失败，这是**符合预期的行为**。

要在当前目录下构建可用的 C 动态库：

```sh
cargo build --features _cdylib
```

如果要在 workspace 目录下构建，必须指定`package`：

```sh
cargo build -p sqlite_ext_ntqq_db --features _cdylib
```
