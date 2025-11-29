import argparse
import sys
from ieapp.workspace import create_workspace, WorkspaceExistsError
from ieapp.notes import create_note, NoteExistsError
from ieapp.logging_utils import setup_logging

DEFAULT_NOTE_CONTENT = "# New Note\n"


def main() -> None:
    setup_logging()

    parser = argparse.ArgumentParser(description="IEapp CLI")
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # create-workspace command
    cw_parser = subparsers.add_parser("create-workspace", help="Create a new workspace")
    cw_parser.add_argument("root_path", help="Root path for workspaces")
    cw_parser.add_argument("workspace_id", help="ID of the workspace to create")

    # note command
    note_parser = subparsers.add_parser("note", help="Note management")
    note_subparsers = note_parser.add_subparsers(
        dest="note_command", help="Note commands"
    )

    # note create command
    nc_parser = note_subparsers.add_parser("create", help="Create a new note")
    nc_parser.add_argument(
        "workspace_path", help="Full path to the workspace directory"
    )
    nc_parser.add_argument("note_id", help="ID of the note to create")
    nc_parser.add_argument(
        "--content", help="Content of the note", default=DEFAULT_NOTE_CONTENT
    )

    args = parser.parse_args()

    if args.command == "create-workspace":
        try:
            create_workspace(args.root_path, args.workspace_id)
            print(
                f"Workspace '{args.workspace_id}' created successfully at '{args.root_path}'"
            )
        except WorkspaceExistsError as e:
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(1)
        except Exception as e:
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(1)

    elif args.command == "note":
        if args.note_command == "create":
            try:
                create_note(args.workspace_path, args.note_id, args.content)
                print(f"Note '{args.note_id}' created successfully.")
            except NoteExistsError as e:
                print(f"Error: {e}", file=sys.stderr)
                sys.exit(1)
            except Exception as e:
                print(f"Error: {e}", file=sys.stderr)
                sys.exit(1)
        else:
            note_parser.print_help()

    else:
        parser.print_help()


if __name__ == "__main__":
    main()
