# 一键解密 NTQQ 数据库！


## 食用说明

从 [release](https://github.com/artiga033/ntdb_unwrap/releases) 下载对应平台的可执行文件。

或者，
```sh
cargo install ntdb_unwrap-cli
```

### (Rooted?) Android

**提示：** 如果你使用了`-N` 参数（即程序不会尝试先复制数据库到临时文件，再去操作临时文件），则有可能损坏数据库文件，同时，建议启用此选项时先强行停止QQ进程。

以 root 权限（或者其它能使此程序有权访问`/data/user/0/com.tencent.mobileqq/`的办法）直接运行即可。

### Windows (实验性)

直接运行并按提示操作即可，你可能需要先退出已登录的账号。本程序会启动一个新的QQ进程，你需要登录对应的账号，以便提取数据库密钥。

另外，由于需要二进制分析和动态调试，实际情况根据CPU架构有所不同：

- x86_64: 🟢支持
- x86: 🔴不支持
- ARM64: 🟡计划支持

### 其他平台

目前不支持自动解密，请手动获取数据库密钥，然后通过命令行参数指定。

### 大于 1 GiB 的数据库（实验性兼容）

参见 [原 issue #58](https://github.com/QQBackup/qq-win-db-key/issues/58)。NTQQ 的 1024 字节文件头使物理 1 GiB 对应的 SQLite 逻辑位置提前了 1024 字节。部分数据库在此位置保留页，与标准 SQLite 的逻辑 1 GiB 保留页相差一页（4096 字节页大小），因此超过该边界后可能报数据库损坏。

遇到此问题时，可给 `export` 或 `serve` 添加实验性参数 `--pending-byte-offset=-1024`，将源库的保留页位置提前 1024 字节以匹配其布局。默认不启用；导出的明文库仍使用标准 SQLite 布局。

## 作为 Rust crate 使用

![docs.rs](https://img.shields.io/docsrs/ntdb_unwrap)

## 另见

用于直接读取 ntqq 数据库的 [SQLite VFS扩展](./sqlite_extension/)

## Credits

- [QQBackup/qq-win-db-key](https://github.com/QQBackup/qq-win-db-key)
- [mobyw/GroupChatAnnualReport](https://github.com/mobyw/GroupChatAnnualReport)
- [rusqlite](https://github.com/rusqlite/rusqlite) and [SQLite](https://sqlite.org)