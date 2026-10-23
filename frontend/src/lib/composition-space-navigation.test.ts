import { describe, expect, it } from "vitest";
import { route as historyRoute } from "../routes/spaces/[space_id]/compositions/[composition_id]/history";
import { route as revisionLayoutRoute } from "../routes/spaces/[space_id]/compositions/[composition_id]/[revision_id]";
import { route as revisionRoute } from "../routes/spaces/[space_id]/compositions/[composition_id]/[revision_id]/index";
import { route as editRoute } from "../routes/spaces/[space_id]/compositions/[composition_id]/[revision_id]/edit";
import { route as listRoute } from "../routes/spaces/[space_id]/compositions/index";
import { route as newRoute } from "../routes/spaces/[space_id]/compositions/new";

describe("Composition Space navigation", () => {
  it("REQ-FE-073: assigns all Composition routes to Saved tools navigation", () => {
    const routes = [
      listRoute,
      newRoute,
      revisionLayoutRoute,
      revisionRoute,
      editRoute,
      historyRoute,
    ];

    expect(routes.map((route) => route.info?.spaceShell.navigation)).toEqual([
      "compositions",
      "compositions",
      "compositions",
      "compositions",
      "compositions",
      "compositions",
    ]);
  });
});
