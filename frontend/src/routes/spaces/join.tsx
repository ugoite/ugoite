import { A, useSearchParams } from "@solidjs/router";
import { createSignal, Show } from "solid-js";
import { spaceApi } from "~/lib/space-api";

const toMessage = (value: unknown): string => {
  if (value instanceof Error && value.message.trim()) {
    return value.message;
  }
  if (typeof value === "string" && value.trim()) {
    return value;
  }
  return "Failed to accept invitation.";
};

export default function SpaceInvitationJoinRoute() {
  const [searchParams] = useSearchParams();
  const [spaceId, setSpaceId] = createSignal(searchParams.space_id || "");
  const [token, setToken] = createSignal(searchParams.token || "");
  const [isSubmitting, setIsSubmitting] = createSignal(false);
  const [error, setError] = createSignal("");
  const [joinedSpaceId, setJoinedSpaceId] = createSignal("");
  const [joinedMemberId, setJoinedMemberId] = createSignal("");
  const [joinedRole, setJoinedRole] = createSignal("");
  const [joinedState, setJoinedState] = createSignal("");

  const handleSubmit = async (event: Event) => {
    event.preventDefault();
    const nextSpaceId = spaceId().trim();
    const nextToken = token().trim();
    if (!nextSpaceId) {
      setError("Space ID is required.");
      return;
    }
    if (!nextToken) {
      setError("Invitation token is required.");
      return;
    }

    setIsSubmitting(true);
    setError("");
    setJoinedSpaceId("");
    setJoinedMemberId("");
    setJoinedRole("");
    setJoinedState("");

    try {
      const result = await spaceApi.acceptInvitation(nextSpaceId, {
        token: nextToken,
      });
      setJoinedSpaceId(nextSpaceId);
      setJoinedMemberId(result.member.user_id);
      setJoinedRole(result.member.role);
      setJoinedState(result.member.state);
    } catch (value) {
      setError(toMessage(value));
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <main class="mx-auto max-w-2xl ui-page ui-stack">
      <section class="ui-card ui-stack">
        <div>
          <h1 class="ui-page-title">Join a space</h1>
          <p class="ui-page-subtitle mt-2">
            Accept an invitation token from your admin and join the space in
            this browser session.
          </p>
        </div>

        <p class="text-sm ui-muted">
          Paste the space ID the admin shared with you and the invitation token
          from the same invite. The token proves the invite; your current
          browser session becomes the active member after acceptance.
        </p>

        <form class="ui-stack-sm" onSubmit={handleSubmit}>
          <label class="ui-stack-sm">
            <span class="text-sm font-medium">Space ID</span>
            <input
              class="ui-input"
              type="text"
              value={spaceId()}
              onInput={(event) => setSpaceId(event.currentTarget.value)}
              placeholder="e.g. team-notes"
              autocomplete="off"
            />
          </label>
          <label class="ui-stack-sm">
            <span class="text-sm font-medium">Invitation token</span>
            <textarea
              class="ui-input min-h-32 font-mono text-sm"
              value={token()}
              onInput={(event) => setToken(event.currentTarget.value)}
              placeholder="Paste the invitation token here"
              autocomplete="off"
              spellcheck={false}
            />
          </label>
          <button
            type="submit"
            class="ui-button ui-button-primary w-fit"
            disabled={!spaceId().trim() || !token().trim() || isSubmitting()}
          >
            {isSubmitting() ? "Accepting..." : "Accept invitation"}
          </button>
        </form>

        <Show when={error()}>
          <p class="ui-alert ui-alert-error text-sm">{error()}</p>
        </Show>

        <Show when={joinedSpaceId()}>
          <div class="ui-alert ui-alert-success ui-stack-sm text-sm">
            <p>
              Invitation accepted. You are now an active member of{" "}
              <code>{joinedSpaceId()}</code>.
            </p>
            <Show when={joinedMemberId()}>
              <p>
                Joined as <code>{joinedMemberId()}</code> with{" "}
                <code>{joinedRole()}</code> access. Current membership state:
                {" "}
                <code>{joinedState()}</code>.
              </p>
            </Show>
            <A
              href={`/spaces/${joinedSpaceId()}/dashboard`}
              class="ui-button ui-button-primary w-fit"
            >
              Open space dashboard
            </A>
          </div>
        </Show>

        <div class="flex flex-wrap gap-3">
          <A href="/spaces" class="ui-button ui-button-secondary">
            Back to Spaces
          </A>
        </div>
      </section>
    </main>
  );
}
