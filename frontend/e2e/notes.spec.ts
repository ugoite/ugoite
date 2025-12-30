import { test, expect } from "@playwright/test";

test.describe("Notes Page", () => {
	test.beforeEach(async ({ page }) => {
		// Navigate to the notes page
		await page.goto("/notes");
	});

	test("should display the notes page with sidebar", async ({ page }) => {
		// Check main layout elements
		await expect(page.locator("text=IEapp")).toBeVisible();
		await expect(page.locator("text=New Note")).toBeVisible();
		await expect(page.locator("text=List")).toBeVisible();
		await expect(page.locator("text=Canvas")).toBeVisible();
	});

	test("should show empty state when no notes exist", async ({ page }) => {
		await expect(page.locator("text=Select a note to edit")).toBeVisible();
	});

	test("should create a new note", async ({ page }) => {
		// Click new note button
		await page.click("text=New Note");

		// Handle the prompt dialog
		page.once("dialog", async (dialog) => {
			await dialog.accept("My Test Note");
		});

		// Trigger the click that causes the dialog
		await page.click("text=New Note");

		// Wait for the note to appear in the list
		await expect(page.locator("text=My Test Note")).toBeVisible({ timeout: 5000 });
	});

	test("should select and edit a note", async ({ page }) => {
		// First create a note
		page.once("dialog", async (dialog) => {
			await dialog.accept("Editable Note");
		});
		await page.click("text=New Note");

		// Wait for note to be created
		await expect(page.locator("text=Editable Note")).toBeVisible({ timeout: 5000 });

		// Click on the note to select it
		await page.click("text=Editable Note");

		// Editor should now be visible
		const editor = page.locator("textarea");
		await expect(editor).toBeVisible();

		// Clear and type new content
		await editor.fill("# Updated Title\n\n## Status\nCompleted\n\n## Priority\nHigh");

		// Should show unsaved indicator
		await expect(page.locator("text=Unsaved changes")).toBeVisible();

		// Save the note
		await page.click("button:has-text('Save')");

		// Wait for save to complete
		await expect(page.locator("text=Unsaved changes")).not.toBeVisible({ timeout: 5000 });
	});

	test("should extract headers as properties after save", async ({ page }) => {
		// Create a note with structured content
		page.once("dialog", async (dialog) => {
			await dialog.accept("Meeting Note");
		});
		await page.click("text=New Note");

		await expect(page.locator("text=Meeting Note")).toBeVisible({ timeout: 5000 });
		await page.click("text=Meeting Note");

		const editor = page.locator("textarea");
		await editor.fill(
			"# Project Meeting\n\n## Date\n2025-01-15\n\n## Attendees\nAlice, Bob\n\n## Decisions\n- Approved budget",
		);

		await page.click("button:has-text('Save')");

		// Wait for save and refetch
		await page.waitForTimeout(500);

		// Check that properties are displayed in the note card
		const noteCard = page.locator('[data-testid="note-item"]').first();
		await expect(noteCard.locator("text=Date")).toBeVisible();
		await expect(noteCard.locator("text=2025-01-15")).toBeVisible();
	});

	test("should switch between list and canvas view", async ({ page }) => {
		// Start in list view
		await expect(page.locator("text=Select a note to edit")).toBeVisible();

		// Switch to canvas view
		await page.click("button:has-text('Canvas')");

		// Canvas placeholder should be visible
		await expect(page.locator('[data-testid="canvas-placeholder"]')).toBeVisible();
		await expect(page.locator("text=Milestone 6")).toBeVisible();

		// Switch back to list view
		await page.click("button:has-text('List')");
		await expect(page.locator("text=Select a note to edit")).toBeVisible();
	});

	test("should show preview mode in editor", async ({ page }) => {
		// Create a note
		page.once("dialog", async (dialog) => {
			await dialog.accept("Preview Test");
		});
		await page.click("text=New Note");

		await expect(page.locator("text=Preview Test")).toBeVisible({ timeout: 5000 });
		await page.click("text=Preview Test");

		// Editor should be visible
		await expect(page.locator("textarea")).toBeVisible();

		// Click preview button
		await page.click("button:has-text('Preview')");

		// Preview should show rendered content
		const preview = page.locator(".preview");
		await expect(preview).toBeVisible();

		// Switch back to edit
		await page.click("button:has-text('Edit')");
		await expect(page.locator("textarea")).toBeVisible();
	});

	test("should delete a note", async ({ page }) => {
		// Create a note
		page.once("dialog", async (dialog) => {
			await dialog.accept("Delete Me");
		});
		await page.click("text=New Note");

		await expect(page.locator("text=Delete Me")).toBeVisible({ timeout: 5000 });
		await page.click("text=Delete Me");

		// Handle confirmation dialog
		page.once("dialog", async (dialog) => {
			await dialog.accept();
		});

		// Click delete button
		await page.click('[aria-label="Delete note"]');

		// Note should be removed
		await expect(page.locator("text=Delete Me")).not.toBeVisible({ timeout: 5000 });
	});

	test("should display notes in canvas view", async ({ page }) => {
		// Create a note first
		page.once("dialog", async (dialog) => {
			await dialog.accept("Canvas Note");
		});
		await page.click("text=New Note");

		await expect(page.locator("text=Canvas Note")).toBeVisible({ timeout: 5000 });

		// Switch to canvas
		await page.click("button:has-text('Canvas')");

		// Note should appear as a card in canvas
		const canvasCard = page.locator('[data-testid="canvas-note-card"]');
		await expect(canvasCard).toBeVisible();
		await expect(canvasCard.locator("text=Canvas Note")).toBeVisible();
	});

	test("should use keyboard shortcut to save", async ({ page }) => {
		// Create a note
		page.once("dialog", async (dialog) => {
			await dialog.accept("Keyboard Save");
		});
		await page.click("text=New Note");

		await expect(page.locator("text=Keyboard Save")).toBeVisible({ timeout: 5000 });
		await page.click("text=Keyboard Save");

		const editor = page.locator("textarea");
		await editor.fill("# Updated via keyboard");

		// Should show unsaved
		await expect(page.locator("text=Unsaved changes")).toBeVisible();

		// Use Cmd/Ctrl + S
		await editor.press("Meta+s");

		// Should save (unsaved indicator gone)
		await expect(page.locator("text=Unsaved changes")).not.toBeVisible({ timeout: 5000 });
	});
});
