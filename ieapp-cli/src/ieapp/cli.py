import argparse
import sys
from ieapp.workspace import create_workspace, WorkspaceExistsError
from ieapp.logging_utils import setup_logging


def main() -> None:
    setup_logging()

    parser = argparse.ArgumentParser(description="IEapp CLI")
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # create-workspace command
    cw_parser = subparsers.add_parser("create-workspace", help="Create a new workspace")
    cw_parser.add_argument("root_path", help="Root path for workspaces")
    cw_parser.add_argument("workspace_id", help="ID of the workspace to create")

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
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
