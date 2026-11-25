/** Request-scoped authorized schema; the owner guards SQL, parameters and host identity. */
export interface PlannedSqlColumns {
  columns: readonly string[];
  /** Typed result projection returned by the authorized Rust planner. */
  result_columns?: readonly {
    name: string;
    value_type: {
      scalar_type: import("~/lib/composition-variable-binding").CompositionScalarType;
      nullable: boolean;
    };
  }[];
  isCurrent: () => boolean | Promise<boolean>;
}
export type PlanSqlColumns = () => Promise<PlannedSqlColumns>;

/** Eligibility only. Form metadata cannot establish wildcard output names or order. */
