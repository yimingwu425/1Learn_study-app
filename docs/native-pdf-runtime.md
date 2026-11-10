# 原生 PDF 运行说明

当前新版启动、题库、8 项 Agent、组卷、试卷篮、完整原页包、识别来源校验和区域证据均由 Rust 路径执行，不需要 Python/旧 Node 兼容服务。

## 引擎与边界

- PdfCraft CLI 0.20.0（Rust 项目 storytold/pdfcraft）处理解析、渲染、文本位置、红删、裁页、抽页、矢量 PDF 贴入和展平；没有自行重写 PDF 解析器或渲染器。
- Rust 负责业务选题规则、可信来源重取、布局、JSON Schema、文件哈希、独立复核门槛和 ZIP。
- printpdf 0.8.2 仅生成自己的封面、题号、标题及笔记；lopdf 0.35.0 仅清理生成包的互动引用，不改原 PDF。
- 本机 Tesseract 5.5.3 只用于损坏/不完整文字层的导出边界定位。不会把其转写自动入库；AI 题目识别模型仍未选择或连接。
- 精确组卷与「完整原页」资料包保持不同格式。后者明确提示同页可能含相邻题及共用材料。
- 原文件只读。红删/裁切在工作副本执行；原始文件、用户题目编辑、笔记及旧项目不覆盖。

## 路径与配置

默认引擎：`.toolchain/pdfcraft-cli/bin/pdfcraft-cli-0.20.0-macos-universal/pdfcraft-cli`；可用 `STUDY_PDFCRAFT_BIN` 指定等价官方工具。

边界 OCR 默认 `/opt/homebrew/bin/tesseract`；可用 `STUDY_TESSERACT_BIN` 指定已安装的可信 Tesseract。它是本机原有工具，没有在本轮替换或改变系统设置。

标题/笔记静态字体：`server/assets/fonts/StudyNotoSansSC-Regular.ttf`。上游 Google Fonts Noto Sans SC；OFL 与下载/派生 SHA 记录在同目录。可变字体固化为 wght=400 是一次性开发操作，应用不调用字体转换脚本或 Python。

`PYTHON`、`PYTHON_BIN`、`STUDY_LEGACY_AGENT_URL` 不再影响新版核心。旧工具单独启动，保留历史实现。

## 区域坐标与版本

识别协议继续使用相对原 CropBox 的未旋转点坐标，页码从 1 开始。Rust 在内存工作副本取消页面 Rotate 后验证坐标/文字并生成区域图，不写回来源。

当前区域版本为 `pdfcraft-region-v2`，区域图片的 SHA-256 绑定实际 PNG 字节。旧 `pymupdf-region-v1` 图像复核记录仍保留，但不会作为新版本证据自动通过；需要重新生成区域图并独立复核。独立 run、完整题目摘要、来源角色/身份、图像/引用摘要与各项完整性检查仍全部要求。

导出边界缓存位于数据目录 `renders/geometry-v5`，按来源 SHA 和页码区分，仅保存引擎几何结果。缓存不是题干真值，不覆盖数据库。

## 许可与分发

官方 PdfCraft 包内的 MIT/Apache-2.0、NOTICE、字体与依赖归属必须随工具保留。printpdf/lopdf 为 MIT，jsonschema 和 zip 依项目声明保留许可，Noto 字体为 SIL OFL，Tesseract 为 Apache-2.0。完整许可以随包文件和锁定版本为准。

PyMuPDF 仍存在于开发/旧兼容环境，已脱离新版运行路径。不可据此宣称整个工作区不存在 Python 或忽略旧工具分发的许可要求。

Mac 资源暂存脚本现在复制 Rust 核心、前端、PdfCraft 完整工具包/声明、字体和考纲显示资产、Schema；不复制 Python 助手，不自动打包用户题库。Tesseract 仍使用本机安装，尚未制成含 OCR 的自包含分发包。签名、公证、安装器及其他系统/架构仍待验证。

官方资料：[PdfCraft](https://github.com/storytold/pdfcraft)、[printpdf](https://docs.rs/printpdf/0.8.2/printpdf/)、[jsonschema](https://docs.rs/jsonschema/0.33.0/jsonschema/)、[Noto Sans SC](https://github.com/google/fonts/tree/main/ofl/notosanssc)。
