export default function DeviceApprovalRoute() {
  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 class="ui-page-title">Device authorization is not available</h1>
        <p class="ui-muted">
          CLI OAuth device authorization and agent credentials are future
          functionality. Ugoite v0.1 supports browser Passkey authentication and
          authenticated MCP access with server-issued credentials.
        </p>
      </section>
    </main>
  );
}
