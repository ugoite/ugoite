import json
import logging
import sys
from typing import Any


class JSONFormatter(logging.Formatter):
    """Formatter that outputs JSON strings."""

    def format(self, record: logging.LogRecord) -> str:
        """Formats a log record as a JSON string.

        Args:
            record: The log record to format.

        Returns:
            A JSON string representation of the log record.

        """
        log_record: dict[str, Any] = {
            "timestamp": self.formatTime(record),
            "level": record.levelname,
            "message": record.getMessage(),
            "module": record.module,
            "funcName": record.funcName,
            "lineno": record.lineno,
        }
        if record.exc_info:
            log_record["exception"] = self.formatException(record.exc_info)
        return json.dumps(log_record)


def setup_logging() -> None:
    """Configures the root logger with a JSON formatter on stdout."""
    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(JSONFormatter())
    root = logging.getLogger()
    # Avoid adding multiple handlers if setup is called multiple times
    if not root.handlers:
        root.addHandler(handler)
        root.setLevel(logging.INFO)
