export type DocsiteLocale = "en" | "ja";

export const DOCSITE_I18N: Record<DocsiteLocale, Record<string, string>> = {
	en: {
		brand: "Ugoite Docsite",
		nav_cli: "CLI Guide",
		nav_rest: "REST API",
		nav_mcp: "MCP API",
		theme_label: "Theme",
		mode_label: "Mode",
		language_label: "Language",
		mode_light: "Light",
		mode_dark: "Dark",
		home_title: "Ship with confidence from one source of truth",
		home_desc:
			"Ugoite Docsite renders docs/ directly and organizes architecture, API, requirements, and governance into one readable map.",
		home_link_cli: "CLI Guide",
		home_link_rest: "REST API Spec",
		home_link_mcp: "MCP API Spec",
		source_prefix: "Source:",
	},
	ja: {
		brand: "Ugoite ドキュメント",
		nav_cli: "CLI ガイド",
		nav_rest: "REST API",
		nav_mcp: "MCP API",
		theme_label: "テーマ",
		mode_label: "表示モード",
		language_label: "言語",
		mode_light: "ライト",
		mode_dark: "ダーク",
		home_title: "単一の信頼できる情報源で、確信を持って開発する",
		home_desc:
			"Ugoite Docsite は docs/ を直接レンダリングし、アーキテクチャ・API・要件・ガバナンスを読みやすい情報マップとして整理します。",
		home_link_cli: "CLI ガイド",
		home_link_rest: "REST API 仕様",
		home_link_mcp: "MCP API 仕様",
		source_prefix: "参照元:",
	},
};
