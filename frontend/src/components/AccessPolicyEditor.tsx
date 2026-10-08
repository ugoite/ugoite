import { createEffect, createSignal, For, Show } from "solid-js";
import {
  accessApi,
  type AccessPolicy,
  type ResourceKind,
} from "~/lib/access-api";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { spaceApi } from "~/lib/ugoite-client";
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
  const [members, { refetch: refetchMembers }] = createResource(
    () => props.spaceId,
    async (spaceId) => await spaceApi.listMembers(spaceId),
  );
  const [selectedPrincipalId, setSelectedPrincipalId] = createSignal("");
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
      setSelectedPrincipalId("");
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
    const principal = selectedPrincipalId();
    const selected = actions().split(",").map((action) => action.trim())
      .filter((action) =>
        ["read", "update", "delete", "share"].includes(action)
      ) as AccessPolicy["grants"][number]["actions"];
    if (
      !principal ||
      !members()?.some((member) =>
        member.principal.state === "active" &&
        member.principal.principal_id === principal
      ) ||
      selected.length === 0
    ) return;
    setGrants((current) => [
      ...current.filter((grant) => grant.principal_id !== principal),
      { principal_id: principal, actions: selected },
    ]);
    setSelectedPrincipalId("");
  };

  const eligibleMembers = () =>
    (members() ?? []).filter((member) => member.principal.state === "active");

  const principalOptionPrompt = () =>
    members.loading
      ? t("common.loading")
      : eligibleMembers().length > 0
      ? t("accessPolicy.choosePrincipal")
      : t("accessPolicy.noAvailableMembers");

  const principalLabel = (principalId: string) =>
    members()?.find((member) => member.principal.principal_id === principalId)
      ?.principal.display_name.trim() || t("accessPolicy.unknownPrincipal");

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
          <p>
            {formatUserFacingError(policy.error, "accessPolicy.failedLoad")}
          </p>
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
      <Show when={members.error}>
        <div class="ui-alert ui-alert-error" role="alert">
          <p>
            {formatUserFacingError(
              members.error,
              "accessPolicy.failedLoadMembers",
            )}
          </p>
          <button
            type="button"
            class="ui-button ui-button-secondary mt-2"
            onClick={() => void refetchMembers()}
          >
            {t("common.retry")}
          </button>
        </div>
      </Show>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
        <select
          class="ui-input"
          aria-label={t("accessPolicy.principal")}
          value={selectedPrincipalId()}
          disabled={!canEdit() || members.loading || Boolean(members.error) ||
            eligibleMembers().length === 0}
          aria-busy={members.loading || undefined}
          onChange={(event) =>
            setSelectedPrincipalId(event.currentTarget.value)}
        >
          <option value="">{principalOptionPrompt()}</option>
          <For each={eligibleMembers()}>
            {(member) => (
              <option value={member.principal.principal_id}>
                {member.principal.display_name.trim() ||
                  t("accessPolicy.unknownPrincipal")}
              </option>
            )}
          </For>
        </select>
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
        disabled={!canEdit() || !selectedPrincipalId() || members.loading ||
          Boolean(members.error)}
      >
        {t("accessPolicy.addGrant")}
      </button>
      <For each={grants()}>
        {(grant) => (
          <div class="flex items-center justify-between gap-2">
            <div class="min-w-0">
              <span>{principalLabel(grant.principal_id)}</span>{" "}
              <span>{grant.actions.join(", ")}</span>
              <details class="mt-1">
                <summary>{t("settings.advancedDetails")}</summary>
                <dl>
                  <dt>{t("accessPolicy.principalId")}</dt>
                  <dd class="break-all">
                    <code>{grant.principal_id}</code>
                  </dd>
                </dl>
              </details>
            </div>
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
