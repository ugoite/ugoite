import { useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { SpaceSettings } from "~/components/SpaceSettings";
import { spaceApi } from "~/lib/space-api";
import type { SpaceMember, SpacePatchPayload } from "~/lib/types";

const managedRoles = ["admin", "editor", "viewer"] as const;
type ManagedRole = (typeof managedRoles)[number];

const toMessage = (value: unknown): string => {
	if (typeof value === "string" && value.trim()) return value;
	if (value instanceof Error && value.message.trim()) return value.message;
	return "";
};

const authHintFromError = (value: unknown): string => {
	const message = toMessage(value).toLowerCase();
	if (
		message.includes("401") ||
		message.includes("authentication") ||
		message.includes("unauthorized")
	) {
		return "Authentication required. Configure UGOITE_AUTH_BEARER_TOKEN or UGOITE_AUTH_API_KEY for frontend server process.";
	}
	if (
		message.includes("403") ||
		message.includes("forbidden") ||
		message.includes("not authorized")
	) {
		return "You are signed in but do not have enough permissions for this action.";
	}
	return "";
};

export default function SpaceSettingsRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;

	const [space, { refetch }] = createResource(async () => {
		return await spaceApi.get(spaceId());
	});
	const [members, { refetch: refetchMembers }] = createResource(async () => {
		return await spaceApi.listMembers(spaceId());
	});

	const [inviteUserId, setInviteUserId] = createSignal("");
	const [inviteRole, setInviteRole] = createSignal<ManagedRole>("viewer");
	const [inviteEmail, setInviteEmail] = createSignal("");
	const [inviteToken, setInviteToken] = createSignal("");
	const [memberActionError, setMemberActionError] = createSignal("");
	const [memberActionPending, setMemberActionPending] = createSignal(false);

	const handleSave = async (payload: SpacePatchPayload) => {
		await spaceApi.patch(spaceId(), payload);
		await refetch();
	};

	const handleTestConnection = async (config: Record<string, unknown>) => {
		const uri = typeof config.uri === "string" ? config.uri : "";
		return await spaceApi.testConnection(spaceId(), {
			storage_config: { uri },
		});
	};

	const handleInvite = async () => {
		const userId = inviteUserId().trim();
		if (!userId) {
			setMemberActionError("User ID is required.");
			return;
		}
		setMemberActionPending(true);
		setMemberActionError("");
		setInviteToken("");
		try {
			const response = await spaceApi.inviteMember(spaceId(), {
				user_id: userId,
				role: inviteRole(),
				email: inviteEmail().trim() || undefined,
			});
			setInviteToken(response.invitation.token);
			setInviteUserId("");
			setInviteEmail("");
			await refetchMembers();
		} catch (error) {
			setMemberActionError(toMessage(error) || "Failed to invite member.");
		} finally {
			setMemberActionPending(false);
		}
	};

	const updateRole = async (memberUserId: string, role: ManagedRole) => {
		setMemberActionPending(true);
		setMemberActionError("");
		try {
			await spaceApi.updateMemberRole(spaceId(), memberUserId, { role });
			await refetchMembers();
		} catch (error) {
			setMemberActionError(toMessage(error) || "Failed to update role.");
		} finally {
			setMemberActionPending(false);
		}
	};

	const revokeMember = async (memberUserId: string) => {
		setMemberActionPending(true);
		setMemberActionError("");
		try {
			await spaceApi.revokeMember(spaceId(), memberUserId);
			await refetchMembers();
		} catch (error) {
			setMemberActionError(toMessage(error) || "Failed to revoke member.");
		} finally {
			setMemberActionPending(false);
		}
	};

	const memberAuthHint = createMemo(() => {
		return authHintFromError(memberActionError() || members.error);
	});

	return (
		<SpaceShell spaceId={spaceId()}>
			<div class="mx-auto max-w-5xl ui-stack">
				<div>
					<h1 class="ui-page-title">Space Settings</h1>
					<p class="ui-page-subtitle mt-1">Space ID: {spaceId()}</p>
				</div>

				<div class="ui-card">
					<p class="text-sm ui-muted">
						Localhost and remote mode both require authenticated sessions. For frontend proxy usage,
						configure <code>UGOITE_AUTH_BEARER_TOKEN</code> or <code>UGOITE_AUTH_API_KEY</code>.
					</p>
				</div>

				<div class="mt-2">
					<Show when={space.loading}>
						<p class="text-sm ui-muted">Loading space...</p>
					</Show>
					<Show when={space.error}>
						<p class="text-sm ui-text-danger">Failed to load space.</p>
						<Show when={authHintFromError(space.error)}>
							<p class="text-sm ui-muted mt-1">{authHintFromError(space.error)}</p>
						</Show>
					</Show>
					<Show when={space()}>
						{(ws) => (
							<SpaceSettings
								space={ws()}
								onSave={handleSave}
								onTestConnection={handleTestConnection}
							/>
						)}
					</Show>
				</div>

				<section class="ui-card ui-stack-sm">
					<h2 class="text-lg font-semibold">Members</h2>
					<p class="text-sm ui-muted">
						Invite members, update roles, and revoke access in this space.
					</p>

					<div class="grid grid-cols-1 md:grid-cols-4 gap-2 items-end">
						<label class="ui-stack-sm">
							<span class="text-xs ui-muted">User ID</span>
							<input
								class="ui-input"
								value={inviteUserId()}
								onInput={(event) => setInviteUserId(event.currentTarget.value)}
							/>
						</label>
						<label class="ui-stack-sm">
							<span class="text-xs ui-muted">Role</span>
							<select
								class="ui-select"
								value={inviteRole()}
								onInput={(event) => setInviteRole(event.currentTarget.value as ManagedRole)}
							>
								<For each={managedRoles}>{(role) => <option value={role}>{role}</option>}</For>
							</select>
						</label>
						<label class="ui-stack-sm md:col-span-2">
							<span class="text-xs ui-muted">Email (optional)</span>
							<input
								class="ui-input"
								value={inviteEmail()}
								onInput={(event) => setInviteEmail(event.currentTarget.value)}
							/>
						</label>
					</div>
					<button
						class="ui-button ui-button-primary w-fit"
						onClick={handleInvite}
						disabled={memberActionPending()}
					>
						Invite Member
					</button>

					<Show when={inviteToken()}>
						<div class="ui-alert ui-alert-info text-sm">
							Invitation token (share once): <code>{inviteToken()}</code>
						</div>
					</Show>
					<Show when={memberActionError()}>
						<p class="text-sm ui-text-danger">{memberActionError()}</p>
					</Show>
					<Show when={memberAuthHint()}>
						<p class="text-sm ui-muted">{memberAuthHint()}</p>
					</Show>

					<Show when={members.loading}>
						<p class="text-sm ui-muted">Loading members...</p>
					</Show>
					<Show when={members.error}>
						<p class="text-sm ui-text-danger">
							Failed to load members
							<Show when={toMessage(members.error)}>{`: ${toMessage(members.error)}`}</Show>
						</p>
					</Show>
					<Show when={!members.loading && !members.error && (members() || []).length === 0}>
						<p class="text-sm ui-muted">No members found.</p>
					</Show>
					<div class="ui-stack-sm">
						<For each={members() || []}>
							{(member: SpaceMember) => (
								<div class="ui-card flex flex-col gap-2">
									<div class="flex flex-wrap items-center justify-between gap-2">
										<div>
											<p class="font-medium">{member.user_id}</p>
											<p class="text-xs ui-muted">state: {member.state}</p>
										</div>
										<div class="flex flex-wrap items-center gap-2">
											<Show
												when={member.role !== "owner"}
												fallback={<span class="text-sm ui-muted">owner</span>}
											>
												<select
													class="ui-select"
													value={member.role === "owner" ? "admin" : member.role}
													onInput={(event) =>
														void updateRole(
															member.user_id,
															event.currentTarget.value as ManagedRole,
														)
													}
													disabled={memberActionPending() || member.state !== "active"}
												>
													<For each={managedRoles}>
														{(role) => <option value={role}>{role}</option>}
													</For>
												</select>
											</Show>
											<button
												class="ui-button ui-button-secondary text-sm"
												onClick={() => void revokeMember(member.user_id)}
												disabled={
													memberActionPending() ||
													member.role === "owner" ||
													member.state === "revoked"
												}
											>
												Revoke
											</button>
										</div>
									</div>
								</div>
							)}
						</For>
					</div>
				</section>
			</div>
		</SpaceShell>
	);
}
