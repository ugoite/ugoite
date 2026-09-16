import { expect, type Page } from "@playwright/test";

/** Fail if any rendered text leaked object coercion ("[object Object]"). */
export async function expectNoObjectCoercion(page: Page): Promise<void> {
	const text = await page.evaluate(
		() => document.body?.innerText ?? "",
	);
	expect(text).not.toContain("[object Object]");
}

/**
 * iOS Safari auto-zooms focused text-entry controls smaller than 16px.
 * Assert every visible text input/select/textarea meets the 16px floor.
 * Non-text inputs never trigger auto-zoom and are excluded.
 */
export async function expectMobileControlFontSize(page: Page): Promise<void> {
	const sizes = await page.locator(
		"input:not([type='hidden']):not([type='checkbox']):not([type='radio']):not([type='range']):not([type='color']):not([type='file']):not([type='submit']):not([type='button']):not([type='reset']):not([type='image']), select, textarea",
	).evaluateAll((elements) =>
		elements
			.map((element) => {
				const rect = element.getBoundingClientRect();
				return rect.width > 0 && rect.height > 0
					? Number.parseFloat(getComputedStyle(element).fontSize)
					: null;
			})
			.filter((size): size is number => size !== null)
	);

	// Some responsive screens intentionally have no form controls. The zoom
	// guard applies to controls when present, not to the screen as a whole.
	if (sizes.length === 0) return;
	for (const size of sizes) {
		expect(size).toBeGreaterThanOrEqual(16);
	}
}
