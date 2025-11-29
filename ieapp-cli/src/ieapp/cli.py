"""CLI entry point."""

import argparse
import sys

from ieapp.indexer import Indexer, query_index
from ieapp.logging_utils import setup_logging
from ieapp.notes import NoteExistsError, create_note
from ieapp.workspace import WorkspaceExistsError, create_workspace

DEFAULT_NOTE_CONTENT = "# New Note\n"


def _handle_create_workspace(args: argparse.Namespace) -> int:
    """Handle the create-workspace command."""
    try:
        create_workspace(args.root_path, args.workspace_id)
        sys.stdout.write(
            f"Workspace '{args.workspace_id}' created successfully at "
            f"'{args.root_path}'\n",
        )
    except WorkspaceExistsError as e:
        sys.stderr.write(f"Error: {e}\n")
        return 1
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"Error: {e}\n")
        return 1
    return 0


def _handle_note_create(args: argparse.Namespace) -> int:
    """Handle the note create command."""
    try:
        create_note(
            args.workspace_path,
            args.note_id,
            args.content,
            author=args.author,
        )
        sys.stdout.write(f"Note '{args.note_id}' created successfully.\n")
    except NoteExistsError as e:
        sys.stderr.write(f"Error: {e}\n")
        return 1
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"Error: {e}\n")
        return 1
    return 0


def _handle_index_run(args: argparse.Namespace) -> int:
    """Handle the index run command."""
    try:
        indexer = Indexer(args.workspace_path)
        indexer.run_once()
        sys.stdout.write(
            f"Indexer completed for workspace '{args.workspace_path}'.\n",
        )
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"Error: {e}\n")
        return 1
    return 0


def _handle_query(args: argparse.Namespace) -> int:
    """Handle the query command."""
    try:
        filter_dict: dict[str, str] = {}
        if args.note_class:
            filter_dict["class"] = args.note_class
        if args.tag:
            filter_dict["tag"] = args.tag

        results = query_index(args.workspace_path, filter_dict or None)

        if not results:
            sys.stdout.write("No notes found.\n")
        else:
            for note in results:
                sys.stdout.write(f"- {note.get('id')}: {note.get('title')}\n")
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"Error: {e}\n")
        return 1
    return 0


SubParserMap = dict[str, argparse.ArgumentParser]


def _setup_parser() -> tuple[argparse.ArgumentParser, SubParserMap]:
    """Set up the argument parser and return subparsers for help display."""
    parser = argparse.ArgumentParser(description="IEapp CLI")
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # create-workspace command
    cw_parser = subparsers.add_parser("create-workspace", help="Create a new workspace")
    cw_parser.add_argument("root_path", help="Root path for workspaces")
    cw_parser.add_argument("workspace_id", help="ID of the workspace to create")

    # note command
    note_parser = subparsers.add_parser("note", help="Note management")
    note_subparsers = note_parser.add_subparsers(
        dest="note_command",
        help="Note commands",
    )

    # note create command
    nc_parser = note_subparsers.add_parser("create", help="Create a new note")
    nc_parser.add_argument(
        "workspace_path",
        help="Full path to the workspace directory",
    )
    nc_parser.add_argument("note_id", help="ID of the note to create")
    nc_parser.add_argument(
        "--content",
        help="Content of the note",
        default=DEFAULT_NOTE_CONTENT,
    )
    nc_parser.add_argument("--author", help="Author of the note", default="user")

    # index command
    index_parser = subparsers.add_parser("index", help="Indexer operations")
    index_subparsers = index_parser.add_subparsers(
        dest="index_command",
        help="Indexer commands",
    )

    # index run command
    ir_parser = index_subparsers.add_parser(
        "run",
        help="Run the indexer to rebuild caches",
    )
    ir_parser.add_argument(
        "workspace_path",
        help="Full path to the workspace directory",
    )

    # query command
    query_parser = subparsers.add_parser("query", help="Query the index")
    query_parser.add_argument(
        "workspace_path",
        help="Full path to the workspace directory",
    )
    query_parser.add_argument(
        "--class",
        dest="note_class",
        help="Filter by class",
    )
    query_parser.add_argument(
        "--tag",
        help="Filter by tag",
    )

    subparser_map = {
        "note": note_parser,
        "index": index_parser,
    }

    return parser, subparser_map


def main() -> None:
    """Entry point for the IEapp CLI."""
    setup_logging()
    parser, subparser_map = _setup_parser()
    args = parser.parse_args()

    exit_code = 0

    if args.command == "create-workspace":
        exit_code = _handle_create_workspace(args)
    elif args.command == "note":
        if args.note_command == "create":
            exit_code = _handle_note_create(args)
        else:
            subparser_map["note"].print_help()
    elif args.command == "index":
        if args.index_command == "run":
            exit_code = _handle_index_run(args)
        else:
            subparser_map["index"].print_help()
    elif args.command == "query":
        exit_code = _handle_query(args)
    else:
        parser.print_help()

    if exit_code != 0:
        sys.exit(exit_code)


if __name__ == "__main__":
    main()
