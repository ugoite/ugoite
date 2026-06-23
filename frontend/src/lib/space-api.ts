import type {
  Space,
  SpaceMember,
  SpaceMemberAcceptPayload,
  SpaceMemberInvitePayload,
  SpaceMemberInviteResponse,
  SpaceMemberRoleUpdatePayload,
  SpacePatchPayload,
  TestConnectionPayload,
} from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

/** Space API client backed by the shared Rust/WASM protocol. */
export const spaceApi = {
  async list(): Promise<Space[]> {
    return await protocolFetch<Space[]>("space.list");
  },

  async create(name: string): Promise<{ id: string; name: string }> {
    return await protocolFetch<{ id: string; name: string }>("space.create", {}, { name });
  },

  async get(id: string): Promise<Space> {
    return await protocolFetch<Space>("space.get", { space_id: id });
  },

  async patch(id: string, payload: SpacePatchPayload): Promise<Space> {
    return await protocolFetch<Space>("space.patch", { space_id: id }, payload);
  },

  async testConnection(
    id: string,
    payload: TestConnectionPayload,
  ): Promise<{ status: string }> {
    return await protocolFetch<{ status: string }>(
      "space.test_connection",
      { space_id: id },
      payload,
    );
  },

  async listMembers(id: string): Promise<SpaceMember[]> {
    return await protocolFetch<SpaceMember[]>("space.members.list", { space_id: id });
  },

  async inviteMember(
    id: string,
    payload: SpaceMemberInvitePayload,
  ): Promise<SpaceMemberInviteResponse> {
    return await protocolFetch<SpaceMemberInviteResponse>(
      "space.members.invite",
      { space_id: id },
      payload,
    );
  },

  async acceptInvitation(
    id: string,
    payload: SpaceMemberAcceptPayload,
  ): Promise<{ member: SpaceMember }> {
    return await protocolFetch<{ member: SpaceMember }>(
      "space.members.accept",
      { space_id: id },
      payload,
    );
  },

  async updateMemberRole(
    id: string,
    memberUserId: string,
    payload: SpaceMemberRoleUpdatePayload,
  ): Promise<{ member: SpaceMember }> {
    return await protocolFetch<{ member: SpaceMember }>(
      "space.members.update_role",
      { space_id: id, member_user_id: memberUserId },
      payload,
    );
  },

  async revokeMember(
    id: string,
    memberUserId: string,
  ): Promise<{ member: SpaceMember }> {
    return await protocolFetch<{ member: SpaceMember }>("space.members.revoke", {
      space_id: id,
      member_user_id: memberUserId,
    });
  },
};
