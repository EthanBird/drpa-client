from __future__ import annotations

import json
import sys
import threading
from dataclasses import dataclass, field
from typing import Any, TextIO


@dataclass
class EventWriter:
    """Thread-safe JSON Lines writer for the host runtime protocol."""

    stream: TextIO = sys.stdout
    _sequence: int = 0
    _lock: threading.Lock = field(default_factory=threading.Lock)

    def emit(self, event_type: str, **payload: Any) -> None:
        with self._lock:
            self._sequence += 1
            event = {"type": event_type, "sequence": self._sequence, **payload}
            self.stream.write(json.dumps(event, ensure_ascii=True, separators=(",", ":")) + "\n")
            self.stream.flush()
