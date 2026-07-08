from __future__ import annotations

import time


def main(ctx):
    username = ctx.params.get("username", "demo")
    ctx.log.info("Hello Web Bot 启动，用户：%s", username)

    for index in range(1, 6):
        time.sleep(0.2)
        ctx.progress(index * 20, f"正在执行第 {index} 步")
        ctx.log.info("完成步骤 %s/5", index)

    output = ctx.output_file("hello-result.txt", "示例输出")
    output.write_text(f"Hello, {username}!\\n", encoding="utf-8")
    ctx.log.info("输出文件已生成：%s", output)
