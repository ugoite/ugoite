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
		home_title: "Ugoite Documentation",
		home_desc:
			"This site renders the main project docs from the repository docs/ directory.",
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
		home_title: "Ugoite ドキュメント",
		home_desc:
			"このサイトは、リポジトリ内の docs/ ディレクトリにある主要ドキュメントをそのまま表示します。",
		home_link_cli: "CLI ガイド",
		home_link_rest: "REST API 仕様",
		home_link_mcp: "MCP API 仕様",
		source_prefix: "参照元:",
	},
};
