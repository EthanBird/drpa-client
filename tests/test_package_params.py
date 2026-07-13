from drpa_client.core.package_params import PackageParamStore


def test_package_param_store_roundtrip(tmp_path):
    store = PackageParamStore(tmp_path)
    params = {"username": "alice", "headless": True, "loop_count": 3}
    store.save_params("demo_bot", "1.0.0", params)

    loaded = store.get_params("demo_bot", "1.0.0")
    assert loaded == params
    assert store.get_params("missing", "1.0.0") is None
