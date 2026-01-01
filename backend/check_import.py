"""Check that ieapp.utils can be imported and exposes resolve_existing_path.

This module is used as a lightweight debug helper for import issues.
"""

import logging

logger = logging.getLogger(__name__)

try:
    import ieapp.utils  # type: ignore[import]

    logger.info("ieapp.utils found: %s", ieapp.utils)
    logger.info("resolve_existing_path: %s", ieapp.utils.resolve_existing_path)
except ImportError:
    logger.exception("ImportError while importing ieapp.utils")
except AttributeError:
    logger.exception("AttributeError while accessing ieapp.utils.resolve_existing_path")
