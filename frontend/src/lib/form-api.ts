import type { Form, FormCreatePayload } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

/** Form API client backed by the shared Rust/WASM protocol. */
export const formApi = {
  async listTypes(spaceId: string): Promise<string[]> {
    return await protocolFetch<string[]>("form.list_types", { space_id: spaceId });
  },

  async list(spaceId: string): Promise<Form[]> {
    return await protocolFetch<Form[]>("form.list", { space_id: spaceId });
  },

  async get(spaceId: string, formName: string): Promise<Form> {
    return await protocolFetch<Form>("form.get", {
      space_id: spaceId,
      form_name: formName,
    });
  },

  async create(spaceId: string, payload: FormCreatePayload): Promise<Form> {
    return await protocolFetch<Form>("form.upsert", { space_id: spaceId }, payload);
  },
};
