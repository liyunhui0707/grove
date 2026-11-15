# 本地开发

仓库根目录是唯一产品开发入口，沿用公开 Git 历史。先读 [当前状态](current-status.md)、[产品规格](product-spec.md)、[决策](decisions.md) 和 [Git 工作流](git-workflow.md)。旧发布副本不再用于改代码或手工导出源码。

使用现有 Node/Rust 和 Tauri 平台前提；依赖版本见 npm/Cargo 锁文件。不要擅自安装工具或改变系统隐私设置。

```sh
npm ci
npm run desktop
```

`npm run dev` 是使用模拟 IPC 的浏览器预览，不能验证原生权限、SQLite 或系统解码。默认验收在本机进行，使用隔离生成/许可素材，远程机器需另有授权。

```sh
npm run build
npm test
npm run test:ui
npm run test:rust
npm run check:rust
npm run test:delivery
npm run test:website
```

按改动选择必要检查；不为 Git/文档整理重复整个原生验收。浏览器测试使用已安装 Chrome，不操作个人浏览器会话；退出、权限、升级和真实导出仍需原生证据。后端性能及故障注入用例是显式 opt-in，只能在测试现场运行。

`npm run desktop:package` 生成带来源清单和收据的本地独立候选，写入忽略的 `.local/deliveries/`，不自动上传。`npm run desktop:open` 打开已接受的 `.local/current-trial/Moment Grove.app`，构建本身不切换入口。版本号保持已确认值，新候选用 build ID 与源清单区分。

未来冻结候选时先提交源码，记录 `git rev-parse HEAD`、`git status --porcelain`、版本、build ID、source-manifest 和产物散列；只有同一来源的证据可复用。发行包需要独立安装/回退/下载验收及当前发布授权。已发布包和标签不可覆盖。

运行数据、SQLite/WAL、缓存、快照和操作日志均不进 Git。不要复制活跃 SQLite 文件冒充一致备份；使用 App 快照。用户照片主库保持授权边界，测试评分、同步和文件操作限指定副本。格式及用户操作细节见 [支持范围](format-support.md) 与 [使用说明](user-guide.md)。
