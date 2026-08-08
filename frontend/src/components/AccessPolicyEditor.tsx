import { createEffect, createSignal, For, Show } from "solid-js";
import {
  accessApi,
  type AccessPolicy,
  type ResourceKind,
} from "~/lib/access-api";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";

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
  const [loadedKey, setLoadedKey] = createSignal<string | null>(null);
  const [message, setMessage] = createSignal("");
  const resourceKey = () =>
    `${props.spaceId}\u0000${props.kind}\u0000${props.resourceId}`;
  let observedResourceKey = resourceKey();

  const canEdit = () =>
    loadedKey() === resourceKey() && !policy.loading && !policy.error;

  createEffect(() => {
    const currentResourceKey = resourceKey();
    if (currentResourceKey !== observedResourceKey) {
      observedResourceKey = currentResourceKey;
      setLoadedKey(null);
      setInherit(true);
      setGrants([]);
      setPrincipalId("");
      setActions("read");
      setMessage("");
    }
  });

  createEffect(() => {
    const currentResourceKey = resourceKey();
    const current = policy();
    if (
      loadedKey() === currentResourceKey ||
      policy.loading ||
      policy.error ||
      current === undefined
    ) {
      return;
    }
    setInherit(current?.inherit_space_role ?? true);
    setGrants(current?.grants ?? []);
    setLoadedKey(currentResourceKey);
  });

  const addGrant = () => {
    if (!canEdit()) return;
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
    if (!canEdit()) return;
    setMessage("");
    try {
      await accessApi.put(props.spaceId, props.kind, props.resourceId, {
        policy_id: policy()?.policy_id ?? crypto.randomUUID(),
        inherit_space_role: inherit(),
        grants: grants(),
      });
      setMessage(t("accessPolicy.saved"));
      setLoadedKey(null);
      await refetch();
    } catch (error) {
      setMessage(formatUserFacingError(error, "accessPolicy.failedSave"));
    }
  };

  return (
    <section class="ui-card ui-stack-sm">
      <h2 class="text-lg font-semibold">{t("accessPolicy.heading")}</h2>
      <Show when={policy.error}>
        <div class="ui-alert ui-alert-error" role="alert">
          <p>{formatUserFacingError(policy.error, "accessPolicy.failedLoad")}</p>
          <button
            type="button"
            class="ui-button ui-button-secondary mt-2"
            onClick={() => {
              setLoadedKey(null);
              void refetch();
            }}
          >
            {t("common.retry")}
          </button>
        </div>
      </Show>
      <label class="flex items-center gap-2">
        <input
          type="checkbox"
          checked={inherit()}
          disabled={!canEdit()}
          onChange={(event) => setInherit(event.currentTarget.checked)}
        />
        {t("accessPolicy.inherit")}
      </label>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
        <input
          class="ui-input font-mono"
          placeholder={t("accessPolicy.principalPlaceholder")}
          value={principalId()}
          disabled={!canEdit()}
          onInput={(event) => setPrincipalId(event.currentTarget.value)}
        />
        <input
          class="ui-input"
          placeholder={t("accessPolicy.actionsPlaceholder")}
          value={actions()}
          disabled={!canEdit()}
          onInput={(event) => setActions(event.currentTarget.value)}
        />
      </div>
      <button
        type="button"
        class="ui-button ui-button-secondary w-fit"
        onClick={addGrant}
        disabled={!canEdit()}
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
              disabled={!canEdit()}
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
        disabled={!canEdit()}
      >
        {t("accessPolicy.save")}
      </button>
      <Show when={message()}>
        <p class="ui-muted">{message()}</p>
      </Show>
    </section>
  );
}
