import { createEffect, createSignal, For, Show } from "solid-js";
import {
  accessApi,
  type AccessPolicy,
  type ResourceKind,
} from "~/lib/access-api";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";

export function AccessPolicyEditor(props: {
  spaceId: string;
  kind: ResourceKind;
  resourceId: string;
}) {
  const [policy, { refetch }] = createResource(
    () => [props.spaceId, props.kind, props.resourceId] as const,
    async ([spaceId, kind, resourceId]) =>
      await accessApi.get(spaceId, kind, resourceId),
  );
  const [principalId, setPrincipalId] = createSignal("");
  const [actions, setActions] = createSignal("read");
  const [inherit, setInherit] = createSignal(true);
  const [grants, setGrants] = createSignal<AccessPolicy["grants"]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [message, setMessage] = createSignal("");

  createEffect(() => {
    const current = policy();
    if (loaded() || policy.loading) return;
    setInherit(current?.inherit_space_role ?? true);
    setGrants(current?.grants ?? []);
    setLoaded(true);
  });

  const addGrant = () => {
    const principal = principalId().trim();
    const selected = actions().split(",").map((action) => action.trim())
      .filter((action) =>
        ["read", "update", "delete", "share"].includes(action)
      ) as AccessPolicy["grants"][number]["actions"];
    if (!principal || selected.length === 0) return;
    setGrants((current) => [
      ...current.filter((grant) => grant.principal_id !== principal),
      { principal_id: principal, actions: selected },
    ]);
    setPrincipalId("");
  };

  const save = async () => {
    setMessage("");
    try {
      await accessApi.put(props.spaceId, props.kind, props.resourceId, {
        policy_id: policy()?.policy_id ?? crypto.randomUUID(),
        inherit_space_role: inherit(),
        grants: grants(),
      });
      setMessage(t("accessPolicy.saved"));
      setLoaded(false);
      await refetch();
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : t("accessPolicy.failedSave"),
      );
    }
  };

  return (
    <section class="ui-card ui-stack-sm">
      <h2 class="text-lg font-semibold">{t("accessPolicy.heading")}</h2>
      <label class="flex items-center gap-2">
        <input
          type="checkbox"
          checked={inherit()}
          onChange={(event) => setInherit(event.currentTarget.checked)}
        />
        {t("accessPolicy.inherit")}
      </label>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
        <input
          class="ui-input font-mono"
          placeholder={t("accessPolicy.principalPlaceholder")}
          value={principalId()}
          onInput={(event) => setPrincipalId(event.currentTarget.value)}
        />
        <input
          class="ui-input"
          placeholder={t("accessPolicy.actionsPlaceholder")}
          value={actions()}
          onInput={(event) => setActions(event.currentTarget.value)}
        />
      </div>
      <button
        type="button"
        class="ui-button ui-button-secondary w-fit"
        onClick={addGrant}
      >
        {t("accessPolicy.addGrant")}
      </button>
      <For each={grants()}>
        {(grant) => (
          <div class="flex items-center justify-between gap-2">
            <code>{grant.principal_id}: {grant.actions.join(", ")}</code>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              onClick={() =>
                setGrants((current) =>
                  current.filter((item) =>
                    item.principal_id !== grant.principal_id
                  )
                )}
            >
              {t("common.remove")}
            </button>
          </div>
        )}
      </For>
      <button
        type="button"
        class="ui-button ui-button-primary w-fit"
        onClick={() => void save()}
      >
        {t("accessPolicy.save")}
      </button>
      <Show when={message()}>
        <p class="ui-muted">{message()}</p>
      </Show>
    </section>
  );
}
