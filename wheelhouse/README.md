# DRPA Wheelhouse（旧版兼容）

本目录只服务旧 PySide6 / RPAZ v1 兼容流程，不是 DRPA Next Windows Release 的依赖来源，也不保证内容与当前 sealed runtime 同步。

DRPA Next 使用 [`offline/README.md`](../offline/README.md) 描述的 sealed runtime：CPython、uv、完整 Windows wheel closure、浏览器和清单作为同一个可验证资产构建。新增依赖应修改 `offline/requirements/runtime.txt` 并通过原生 Windows air-gap workflow，不能只向本目录复制 wheel。

旧版目标为 CPython 3.11，历史目录包括：

```text
wheelhouse/common/
wheelhouse/windows-amd64/
wheelhouse/linux-x86_64/
```

其中 Linux 目录不表示 DRPA Next 当前发布 Linux 安装包。除非修复明确的 v1 兼容问题，否则不要扩展本目录。
