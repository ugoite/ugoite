import { protocolFetch } from "./ugoite-client/protocol";

export type ResourceKind =
  | "entry"
  | "asset"
  | "form"
  | "saved_sql"
  | "materialized_view";

export type AccessPolicy = {
  policy_id: string;
  inherit_space_role: boolean;
  grants: Array<{
    principal_id: string;
    actions: Array<"read" | "update" | "delete" | "share">;
  }>;
};

export const accessApi = {
  async get(
    spaceId: string,
    kind: ResourceKind,
    resourceId: string,
  ): Promise<AccessPolicy | null> {
    return await protocolFetch("access.get", {
      space_id: spaceId,
      kind,
      resource_id: resourceId,
    });
  },

  async put(
    spaceId: string,
    kind: ResourceKind,
    resourceId: string,
    policy: AccessPolicy,
  ): Promise<AccessPolicy> {
    return await protocolFetch(
      "access.put",
      { space_id: spaceId, kind, resource_id: resourceId },
      policy,
    );
  },
};
