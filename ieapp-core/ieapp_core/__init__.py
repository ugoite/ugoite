"""ieapp-core: Rust-based core logic and Python bindings."""

from . import _ieapp_core

# Export the docstring from the native module
__doc__ = _ieapp_core.__doc__

# Export all symbols from the native library
# We do this explicitly to help linters and IDEs, or use __all__
if hasattr(_ieapp_core, "__all__"):
    __all__ = _ieapp_core.__all__
else:
    # Fallback: export everything that doesn't start with an underscore
    __all__ = [k for k in _ieapp_core.__dict__ if not k.startswith("_")]

# Inject symbols into the current module's namespace
for name in __all__:
    globals()[name] = getattr(_ieapp_core, name)
