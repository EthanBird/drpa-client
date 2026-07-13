# Bing 每日一图

这是符合 RPaz Schema v2 的 DRPA Next 示例脚本包，用于访问 Bing 每日一图接口，下载图片和 JSON 元数据。它只使用 Python 标准库，不需要额外安装依赖。

参数：

- `market`：地区代码，留空时使用 `zh-CN`
- `image_count`：获取最近几张图片，留空时为 1，范围 1 到 8

输出：

- `bing-daily-images.json`
- 一张或多张 `.jpg` 图片

测试步骤：

1. 在“脚本包”页面选择仓库根目录的 `examples/bing_daily_image.rpaz`。
2. 打开运行工作台，设置 `market=zh-CN`、`image_count=1`。
3. 点击“预检”，再点击“运行任务”。
4. 右侧日志出现“Bing 每日一图下载完成”并列出图片产物路径即表示安装、参数与封装 Python 执行链都工作正常。
