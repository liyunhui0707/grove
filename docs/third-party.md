# 第三方依赖与素材来源

自写代码、程序图与 assets/grove.svg 为 Apache-2.0。没有复制 Lap/digiKam/nomacs 应用代码，没有捆绑 libheif、FFmpeg、AI 模型或字体。
以下列出直接依赖。完整 macOS arm64 Rust 解析依赖及已安装 npm 构建/测试依赖的声明文本、版本和源码链接见根目录 THIRD-PARTY-NOTICES.txt。

| 直接运行依赖 | 版本 | 声明许可 |
|---|---|---|
| React / React DOM | 19.3.1 | MIT |
| @tanstack/react-virtual | 3.14.26 | MIT |
| @tauri-apps/api | 2.12.4 | Apache-2.0 OR MIT |
| lucide-react | 1.70.0 | ISC |
| tauri | 2.12.5 | Apache-2.0 OR MIT |
| tauri-plugin-dialog | 2.8.4 | Apache-2.0 OR MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| rusqlite | 0.41.1 | MIT |
| image | 0.25.10 | MIT OR Apache-2.0 |
| kamadak-exif | 0.6.2 | BSD-2-Clause |
| roxmltree | 0.21.2 | MIT OR Apache-2.0 |
| uuid | 1.29.1 | Apache-2.0 OR MIT |
| walkdir | 2.5.0 | Unlicense/MIT |
| tokio | 1.54.1 | MIT |
| rustix（Unix 文件描述符与不覆盖重命名） | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| sha2（复制导出 SHA-256） | 0.10.9 | MIT OR Apache-2.0 |
| base64（水印预览IPC） | 0.23.1 | MIT OR Apache-2.0 |
| core-foundation（macOS） | 0.10.1 | MIT OR Apache-2.0 |
| core-graphics（macOS） | 0.25.0 | MIT OR Apache-2.0 |
| foreign-types（macOS对象所有权） | 0.5.0 | MIT/Apache-2.0 |
| objc2（macOS对象调用） | 0.6.5 | MIT |
| objc2-foundation（macOS系统废纸篓/稳定文件引用） | 0.3.2 | MIT |

tauri-build 2.7.4：Apache-2.0 OR MIT；Rust 测试依赖 tempfile 3.27.0：MIT OR Apache-2.0。
SQLite 通过 bundled 功能构建，其声明随完整第三方 notices 一起提供。MPL-2.0 依赖保持未修改，notice 提供对应版本源码地址；项目 Apache-2.0 不覆盖第三方组件自身许可。
系统 sips 和系统字体不复制分发；lucide ISC 声明须在正式分发时保留。版本锁在 npm/Cargo lockfile。
水印使用Apple系统ImageIO/CoreGraphics/CoreText，接口核对已安装SDK；上述直接依赖均来自项目既有缓存/锁文件，没有本轮下载或安装。serde_json启用float_roundtrip，保证持久化排版坐标读回后生成相同清单字节；无版本升级或数据库迁移。
发布前对锁定生产 npm 依赖运行 npm audit，并对 macOS Rust 解析依赖查询 OSV；具体结果见本次发布记录。跨平台依赖未作为本轮二进制支持承诺。

SDR预览使用系统ImageIO缩略图和CoreGraphics sRGB画布；系统ColorSync配置用于生成样本比对，不复制分发。透明度白底合成、八种方向与生成HEIC已在Air验证；不捆绑libheif。

首版公开前升级rusqlite至0.41.1/libsqlite3-sys0.39.1，采用已修复RUSTSEC-2026-0459的SQLite；未改资料库schema2。

官网使用原创CSS插画及程序生成素材的浏览器界面截图，不使用私人照片。私人授权照片仅在本机隔离目录中测试，不复制其图像、EXIF、原始文件名或路径到源码、安装包和官网。归属明确的公开样本来源见format-support.md。
