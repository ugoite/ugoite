import { render } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import SpaceFormDetailRoute from "./[form_name]";
const navigate = vi.fn();
vi.mock("@solidjs/router", () => ({ useNavigate: () => navigate, useParams: () => ({ space_id: "demo", form_name: "Daily Notes" }) }));
describe("legacy Form detail URL", () => { it("opens the selected Form in the unified workspace", () => { render(() => <SpaceFormDetailRoute />); expect(navigate).toHaveBeenCalledWith("/spaces/demo/forms?form=Daily%20Notes&tab=entries", { replace: true }); }); });
