import { expect, test } from "bun:test";
import { E2EClient } from "./lib/client";

const client = new E2EClient();

test("canvas view - drag note and persist position", async () => {
	// Create a workspace and note
	const wsRes = await client.postApi("/workspaces", { name: "Canvas Test" });
	const ws = (await wsRes.json()) as { id: string };

	const noteRes = await client.postApi(`/workspaces/${ws.id}/notes`, {
		content: "# Canvas Note\nTest content",
	});
	const note = (await noteRes.json()) as {
		id: string;
		revision_id: string;
		content: string;
	};

	// Update note position (canvas drag-drop)
	const updateRes = await client.putApi(`/workspaces/${ws.id}/notes/${note.id}`, {
		markdown: note.content,
		parent_revision_id: note.revision_id,
		canvas_position: { x: 100, y: 200 },
	});
	expect(updateRes.status).toBe(200);

	// Fetch note and verify position
	const fetchRes = await client.getApi(`/workspaces/${ws.id}/notes/${note.id}`);
	const updated = (await fetchRes.json()) as { canvas_position: { x: number; y: number } };
	expect(updated.canvas_position).toEqual({ x: 100, y: 200 });

	// Cleanup
	await client.deleteApi(`/workspaces/${ws.id}/notes/${note.id}`);
	await client.deleteApi(`/workspaces/${ws.id}`);
});

test("canvas view - create bi-directional link", async () => {
	// Create workspace and two notes
	const wsRes = await client.postApi("/workspaces", { name: "Link Test" });
	const ws = (await wsRes.json()) as { id: string };

	const note1Res = await client.postApi(`/workspaces/${ws.id}/notes`, {
		content: "# Note 1",
	});
	const note1 = (await note1Res.json()) as { id: string };

	const note2Res = await client.postApi(`/workspaces/${ws.id}/notes`, {
		content: "# Note 2",
	});
	const note2 = (await note2Res.json()) as { id: string };

	// Create a link
	const linkRes = await client.postApi(`/workspaces/${ws.id}/links`, {
		source: note1.id,
		target: note2.id,
		kind: "reference",
	});
	expect(linkRes.status).toBe(201);
	const link = (await linkRes.json()) as {
		id: string;
		source: string;
		target: string;
		kind: string;
	};

	expect(link.source).toBe(note1.id);
	expect(link.target).toBe(note2.id);
	expect(link.kind).toBe("reference");

	// List links
	const linksRes = await client.getApi(`/workspaces/${ws.id}/links`);
	const links = (await linksRes.json()) as typeof link[];
	expect(links.length).toBeGreaterThanOrEqual(1);
	expect(links.some((l) => l.id === link.id)).toBe(true);

	// Delete link
	const delRes = await client.deleteApi(`/workspaces/${ws.id}/links/${link.id}`);
	expect(delRes.status).toBe(204);

	const linksAfterRes = await client.getApi(`/workspaces/${ws.id}/links`);
	const linksAfter = (await linksAfterRes.json()) as typeof link[];
	expect(linksAfter.some((l) => l.id === link.id)).toBe(false);

	// Cleanup
	await client.deleteApi(`/workspaces/${ws.id}/notes/${note1.id}`);
	await client.deleteApi(`/workspaces/${ws.id}/notes/${note2.id}`);
	await client.deleteApi(`/workspaces/${ws.id}`);
});

test("search - keyword search finds notes", async () => {
	// Create workspace and notes with searchable content
	const wsRes = await client.postApi("/workspaces", { name: "Search Test" });
	const ws = (await wsRes.json()) as { id: string };

	const note1Res = await client.postApi(`/workspaces/${ws.id}/notes`, {
		content: "# Unique Keyword Alpha\nContent here",
	});
	const note1 = (await note1Res.json()) as { id: string };

	const note2Res = await client.postApi(`/workspaces/${ws.id}/notes`, {
		content: "# Another Note\nUnique Keyword Beta",
	});
	const note2 = (await note2Res.json()) as { id: string };

	const note3Res = await client.postApi(`/workspaces/${ws.id}/notes`, {
		content: "# Unrelated\nSomething else",
	});
	const note3 = (await note3Res.json()) as { id: string };

	// Search for "Unique Keyword"
	const searchRes = await client.getApi(
		`/workspaces/${ws.id}/search?q=${encodeURIComponent("Unique Keyword")}`,
	);
	const results = (await searchRes.json()) as { id: string }[];

	expect(results.length).toBeGreaterThanOrEqual(2);
	const ids = results.map((r) => r.id);
	expect(ids).toContain(note1.id);
	expect(ids).toContain(note2.id);

	// Cleanup
	await client.deleteApi(`/workspaces/${ws.id}/notes/${note1.id}`);
	await client.deleteApi(`/workspaces/${ws.id}/notes/${note2.id}`);
	await client.deleteApi(`/workspaces/${ws.id}/notes/${note3.id}`);
	await client.deleteApi(`/workspaces/${ws.id}`);
});

test("attachments - upload and delete", async () => {
	// Create workspace
	const wsRes = await client.postApi("/workspaces", { name: "Attachment Test" });
	const ws = (await wsRes.json()) as { id: string };

	// Create a test file
	const testContent = "Test file content";
	const formData = new FormData();
	const blob = new Blob([testContent], { type: "text/plain" });
	formData.append("file", blob, "test.txt");

	// Upload attachment
	const uploadRes = await client.fetch(`${client.backendUrl}/workspaces/${ws.id}/attachments`, {
		method: "POST",
		body: formData,
	});
	expect(uploadRes.status).toBe(201);

	const attachment = (await uploadRes.json()) as {
		id: string;
		filename: string;
		mime_type: string;
		size: number;
	};

	expect(attachment.filename).toBe("test.txt");
	expect(attachment.mime_type).toBe("text/plain");
	expect(attachment.size).toBeGreaterThan(0);

	// Delete attachment
	const delRes = await client.deleteApi(`/workspaces/${ws.id}/attachments/${attachment.id}`);
	expect(delRes.status).toBe(204);

	// Cleanup
	await client.deleteApi(`/workspaces/${ws.id}`);
});
