# Bilibili 搜索

这是一个 DrissionPage RPA 示例包，用于打开 Bilibili 搜索页并导出搜索结果。

## 参数

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `keyword` | string | `Python RPA` | 搜索关键词 |
| `max_results` | integer | `10` | 最多导出的结果数量 |
| `headless` | boolean | `false` | 是否无头运行浏览器 |

## 输出

- `bilibili-search-results.json`
- `bilibili-search-results.md`

## 打包

```bash
cd examples/bilibili_search
zip -r ../bilibili_search.rpaz .
```

也可以在仓库根目录运行：

```bash
python3 tools/build_example_packages.py
```
