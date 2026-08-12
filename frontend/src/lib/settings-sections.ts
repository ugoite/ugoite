import type { UiIconName } from "~/components/UiIcon";
import type { TranslationKey } from "./i18n";

export type SettingsSectionId =
  | "general"
  | "members"
  | "agents"
  | "credentials"
  | "storage"
  | "audit";

export const settingsSections: Array<{
  id: SettingsSectionId;
  icon: UiIconName;
  key: TranslationKey;
}> = [
  { id: "general", icon: "settings", key: "settings.section.general" },
  { id: "members", icon: "members", key: "settings.section.members" },
  { id: "agents", icon: "agent", key: "settings.section.agents" },
  {
    id: "credentials",
    icon: "credential",
    key: "settings.section.credentials",
  },
  { id: "storage", icon: "storage", key: "settings.section.storage" },
  { id: "audit", icon: "history", key: "settings.section.audit" },
];
