from __future__ import annotations

import importlib.util
import json
import sys
import traceback
from pathlib import Path

from drpa_client.sdk import Context


def emit(event_type: str, **payload: object) -> None:
    print(json.dumps({"type": event_type, **payload}, ensure_ascii=False), flush=True)


def main() -> int:
    if len(sys.argv) != 2:
        emit("error", message="bootstrap 缺少任务配置路径")
        return 2

    config_path = Path(sys.argv[1])
    with config_path.open("r", encoding="utf-8") as fp:
        config = json.load(fp)

    package_dir = Path(config["package_dir"])
    entry_path = (package_dir / config["entry"]).resolve()
    if package_dir.resolve() not in entry_path.parents and entry_path != package_dir.resolve():
        emit("error", message=f"入口文件越界：{config['entry']}")
        return 2
    if not entry_path.exists():
        emit("error", message=f"入口文件不存在：{config['entry']}")
        return 2

    sys.path.insert(0, str(package_dir))
    ctx = Context(
        run_id=config["run_id"],
        package_id=config["package_id"],
        package_name=config["package_name"],
        params=config.get("params") or {},
        package_dir=package_dir,
        output_dir=Path(config["output_dir"]),
        log_file=Path(config["log_file"]),
        emit=emit,
    )

    try:
        module = _load_entry(entry_path)
        if not hasattr(module, "main"):
            emit("error", message="入口文件必须定义 main(ctx) 函数")
            return 2
        emit("status", value="running", message="任务已启动")
        module.main(ctx)
        emit("status", value="success", message="任务执行完成")
        return 0
    except KeyboardInterrupt:
        emit("status", value="cancelled", message="任务已取消")
        return 130
    except Exception as exc:  # noqa: BLE001 - user scripts must be isolated
        emit("error", message=str(exc), traceback=traceback.format_exc())
        return 1
    finally:
        ctx.close()


def _load_entry(entry_path: Path):
    spec = importlib.util.spec_from_file_location("drpa_user_script", entry_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"无法加载入口文件：{entry_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


if __name__ == "__main__":
    raise SystemExit(main())
