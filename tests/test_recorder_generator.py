import json
import zipfile
from pathlib import Path

from drpa_client.core.recorder import RecorderPackageGenerator, Recording


def test_recorder_generator_creates_reviewable_rpaz(tmp_path):
    fixture = Path(__file__).parent / "fixtures" / "recording_login.json"
    recording = Recording.from_dict(json.loads(fixture.read_text(encoding="utf-8")))

    archive = RecorderPackageGenerator(tmp_path).generate(
        recording,
        package_id="recorded_example_login",
        name="录制流程 - 登录",
    )

    assert archive.exists()
    with zipfile.ZipFile(archive) as package:
        names = set(package.namelist())
        assert "manifest.yaml" in names
        assert "main.py" in names
        assert "recording.json" in names
        assert "selectors.json" in names
        assert "recorder_notes.md" in names

        manifest = package.read("manifest.yaml").decode("utf-8")
        main = package.read("main.py").decode("utf-8")
        notes = package.read("recorder_notes.md").decode("utf-8")

    assert "recorder:" in manifest
    assert "requires_review: true" in manifest
    assert "name: password" in manifest
    assert "type: password" in manifest
    assert "ctx.params.get('password'" in main
    assert "TODO" in main
    assert "敏感字段" in notes
