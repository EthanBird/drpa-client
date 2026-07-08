from drpa_client.core.manifest import ManifestError, parse_manifest


def test_parse_manifest_with_offline_dependencies():
    manifest = parse_manifest(
        {
            "id": "invoice_downloader",
            "name": "发票下载机器人",
            "version": "1.0.0",
            "entry": "main.py",
            "runtime": {"python": ">=3.11", "isolation": "venv"},
            "dependencies": {
                "strategy": "offline-first",
                "pip": ["DrissionPage"],
                "local": {
                    "common": ["wheels/common/*.whl"],
                    "windows": ["wheels/windows/*.whl"],
                    "linux": ["wheels/linux/*.whl"],
                },
            },
            "params": [
                {"name": "username", "label": "用户名", "type": "string", "required": True},
                {"name": "headless", "label": "无头模式", "type": "boolean", "default": True},
            ],
        }
    )

    assert manifest.id == "invoice_downloader"
    assert manifest.dependencies.pip == ("DrissionPage",)
    assert manifest.dependencies.local_linux == ("wheels/linux/*.whl",)
    assert manifest.params[1].default is True


def test_parse_manifest_requires_entry():
    try:
        parse_manifest({"id": "x", "name": "x", "version": "1.0.0"})
    except ManifestError as exc:
        assert "entry" in str(exc)
    else:
        raise AssertionError("ManifestError was not raised")
