from collections.abc import Awaitable

def list_workspaces(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[str]]: ...
def create_workspace(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def get_workspace(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def patch_workspace(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def test_storage_connection(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def create_note(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def list_notes(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[dict[str, object]]]: ...
def get_note(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def update_note(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def delete_note(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def get_note_history(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def get_note_revision(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def restore_note(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def list_classes(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[dict[str, object]]]: ...
def list_column_types(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[str]]: ...
def get_class(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def upsert_class(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def save_attachment(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def list_attachments(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[dict[str, object]]]: ...
def delete_attachment(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def create_link(
    *args: object,
    **kwargs: object,
) -> Awaitable[dict[str, object]]: ...
def list_links(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[dict[str, object]]]: ...
def delete_link(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def query_index(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[dict[str, object]]]: ...
def search_notes(
    *args: object,
    **kwargs: object,
) -> Awaitable[list[dict[str, object]]]: ...
def extract_properties(
    *args: object,
    **kwargs: object,
) -> dict[str, object]: ...
def validate_properties(
    *args: object,
    **kwargs: object,
) -> tuple[dict[str, object], list[dict[str, object]]]: ...
def build_response_signature(
    *args: object,
    **kwargs: object,
) -> Awaitable[tuple[str, str]]: ...
def migrate_class(
    *args: object,
    **kwargs: object,
) -> Awaitable[int]: ...
def reindex_all(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def update_note_index(
    *args: object,
    **kwargs: object,
) -> Awaitable[None]: ...
def load_hmac_material(
    *args: object,
    **kwargs: object,
) -> Awaitable[tuple[str, bytes]]: ...
