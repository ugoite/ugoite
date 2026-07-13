import { useNavigate, useParams } from "@solidjs/router";
import { onMount } from "solid-js";

export default function SpaceFormDetailRoute() {
  const params = useParams<{ space_id: string; form_name: string }>();
  const navigate = useNavigate();
  onMount(() => navigate(`/spaces/${params.space_id}/forms?form=${encodeURIComponent(params.form_name)}&tab=entries`, { replace: true }));
  return null;
}
