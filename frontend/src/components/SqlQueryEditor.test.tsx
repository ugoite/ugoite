import { describe, expect, it, vi } from "vitest";
import { render } from "@solidjs/testing-library";
import { SqlQueryEditor } from "./SqlQueryEditor";
import { buildSqlSchema } from "~/lib/sql";

describe("REQ-FE-036: SQL Query Editor", () => {
	it("should render without runtime errors", () => {
		const onDiagnostics = vi.fn();
		const result = render(() => (
			<SqlQueryEditor
				value="SELECT * FROM notes"
				onChange={() => undefined}
				schema={buildSqlSchema([])}
				onDiagnostics={onDiagnostics}
			/>
		));
		expect(onDiagnostics).toHaveBeenCalled();
		result.unmount();
	});
});
