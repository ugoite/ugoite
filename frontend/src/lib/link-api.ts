const linksRemovedError = () =>
	new Error("Links API has been removed. Use row_reference fields instead.");

/** Link API client (removed). */
export const linkApi = {
	async create(
		_spaceId: string,
		_payload: { source: string; target: string; kind: string },
	): Promise<never> {
		throw linksRemovedError();
	},

	async list(_spaceId: string): Promise<never> {
		throw linksRemovedError();
	},

	async delete(_spaceId: string, _linkId: string): Promise<never> {
		throw linksRemovedError();
	},
};
