"""Check that ugoite.utils can be imported and exposes resolve_existing_path.

This module is used as a lightweight debug helper for import issues.
"""

import logging

logger = logging.getLogger(__name__)

try:
    import ugoite.utils  # type: ignore[import]

    logger.info("ugoite.utils found: %s", ugoite.utils)
    logger.info("resolve_existing_path: %s", ugoite.utils.resolve_existing_path)
except ImportError:
    logger.exception("ImportError while importing ugoite.utils")
except AttributeError:
    logger.exception(
        "AttributeError while accessing ugoite.utils.resolve_existing_path",
    )
