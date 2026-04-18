
# ars-dl

ars-dl 是一款用 Rust 编写的轻量级命令行下载工具，具有以下特性：

- **多线程分块下载**：支持断点续传，大文件自动使用多个并发连接提升下载速度
- **批量下载**：支持范围模式匹配多个文件，自动跳过已存在的文件
- **智能镜像**：GitHub 链接自动尝试内置镜像（gh-proxy.com 与 ghfast.top）
- **实时进度**：显示下载进度、百分比、速度和预计剩余时间

## 下载

[ars-dl-x86_64-pc-windows-msvc.zip](https://github.com/classronin/ars-dl/releases/latest/download/ars-dl-x86_64-pc-windows-msvc.zip)

> 提示：可以将 `ars-dl.exe` 重命名为您喜欢的名称（如 `a.exe`），并将其添加到 PATH 环境变量中以便全局使用。

## 用法

### 单文件下载

```bash
a <URL> [保存名称]
a "https://example.com/video.mp4"
a "https://example.com/video.mp4" 我的视频
a "https://example.com/video.mp4" "我的视频"
```

### 批量下载

```bash
a <URL模式> [保存文件夹]
a "https://example.com/pic[001-100].jpg" 图片
a "https://example.com/file[0-100:2].png"
```

#### 批量模式说明

- 支持范围表达式 `[起始-结束]` 或 `[起始-结束:步长]`
- 自动识别零填充宽度
- `pic[1-100].jpg` 会匹配 `pic1.jpg` 到 `pic100.jpg`
- `file[001-100].jpg` 会匹配 `file001.jpg` 到 `file100.jpg`
- `file[0-100:2].png` 会匹配 `file0.png`、`file2.png`、`file4.png`...（步长为2）

## 功能特性

### 单文件下载

- 多线程分块下载（8个并发连接）
- 支持断点续传
- 自动检测已下载部分并继续下载
- 实时显示下载进度、百分比、速度和预计剩余时间

### 批量下载

- 10个并发任务同时下载
- 自动跳过已存在的文件
- 简洁的汇总输出

## 输出示例

```
单文件进度：[62.5MB/328.0MB] 19% 23.8MB/s 00:19
单文件完成：Done in 2m34s
批量进度：[99/100] 99% 5.2MB/s
批量汇总：Succ:100 Fail:0 Skip:0 4s
```

## 编译

```bash
cargo build --release
```

## 许可证

MIT OR Apache-2.0


