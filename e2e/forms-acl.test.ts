import { test } from "@playwright/test";

test.describe("Planned Form ACL", () => {
	test.skip(
		"Enforces Form ACL Read/Write Restrictions for Space Members",
		async () => {
			// REQ-SEC-006 planned
		},
	);

	test.skip(
		"Enforces Materialized View Policy Intersection with Form ACL",
		async () => {
			// REQ-SEC-006 planned
		},
	);
});
