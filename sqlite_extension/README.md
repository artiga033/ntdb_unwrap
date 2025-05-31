此 crate 可作为 Rust 库或 C 动态库使用。

## 用法

### 作为 Rust 库

将此 crate 添加为依赖项并调用 `register_offset_vfs` 即可，vfs的名称为常量`OFFSET_VFS_NAME`。
```sh
cargo add sqlite_ext_ntqq_db
```

### 作为 SQLite Runtime Loadable Extension

1. 自行编译或下载预编译得到的对应平台的动态链接库文件（由于此 package 很少有变动，因此 ci 不会发布，并非每次 release 都有动态链接库构建，[目前的最新发布在此下载](https://github.com/artiga033/ntdb_unwrap/releases/tag/v0.1.0)）

2. 在 sqlite (实则 sqlcipher )命令行中加载并打开数据库文件即可。

请注意此扩展的设计目标为打开NTQQ数据库，即检测到NTQQ数据库文件特征时自动偏移1024个字节，其他情况会 fallback 到默认vfs。
这一行为通常不会有问题，不过**仍然建议您在操作正常数据库时不要加载此扩展**。

另外，由于 SQLite 扩展的[函数入口点名称与库文件名强相关](https://www.sqlite.org/loadext.html#:~:text=If%20your%20shared%20library%20ends%20up%20being%20named%20%22YourCode.so%22%20or%20%22YourCode.dll%22%20or%20%22YourCode.dylib%22%20as%20shown%20in%20the%20compiler%20examples%20above%2C%20then%20the%20correct%20entry%20point%20name%20would%20be%20%22sqlite3_yourcode_init%22.)，因此请不要修改文件名。

#### 示例

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

**DB Browser for SQLite**:

参见 <https://github.com/QQBackup/qq-win-db-key/issues/55#issue-2851046321>

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
