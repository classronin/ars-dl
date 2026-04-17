
ars-dl 是一款用 Rust 编写的轻量级命令行下载工具。

它支持单文件多线程分块下载（动态分块大小），能够有效提升大文件下载速度；
同时支持批量下载，通过范围模式匹配多个文件。

把ars-dl.exe改为你偏好名称，例如我更改为a.exe并且纳入PATH环境变量。

下载：[ars-dl-x86_64-pc-windows-msvc.zip]（https://github.com/classronin/ars-dl/releases/download/v0.0.1/ars-dl-x86_64-pc-windows-msvc.zip）

用法：
```
a <URL> [保存名称或文件夹]
a "https://example.com/video.mp4"
a "https://example.com/video.mp4" 我的视频
a "https://example.com/pic[001-100].jpg" 图片
a "https://example.com/file[0-100:2].png"
```

批量模式：
支持范围表达式 [起始-结束] 或 [起始-结束:步长]，自动识别零填充宽度。
例如：pic[1-100].jpg 会匹配 pic1.jpg 到 pic100.jpg
file[001-100].jpg 会匹配 file001.jpg 到 file100.jpg

功能特性：
单文件下载
- 多线程分块下载，大文件自动使用独立连接突破限速
- 动态调整分块数量，兼顾小文件效率与大文件速度
- 失败时自动降级为单流下载
- GitHub 链接自动尝试内置镜像（gh-proxy.com 与 ghfast.top）
- 实时显示下载进度、百分比、平均速度
批量下载
- 10 个并发任务同时下载
- 自动跳过已存在的文件
- 简洁的汇总输出

编译：
```
cargo build --release
```

输出示例：
单文件进度：[62.5MB/328.0MB] 19% 23.8MB/s
单文件完成：Done in 2m34s
批量进度：[99/100] 99% 5.2MB/s
批量汇总：Succ:100 Fail:0 Skip:0 4s

临时文件存放在系统临时目录，下载完成后自动合并并清理。



