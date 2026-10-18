//! Function metadata for SQL expressions that Ugoite exposes to user queries.
//!
//! This module is transport- and engine-independent. A storage adapter may
//! implement a descriptor with a bounded engine function, but the descriptor
//! itself does not register or authorize runtime functions.

/// One admitted SQL aggregate and its syntax, input, and result contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqlAggregateFunction {
    /// The canonical function name used by the SQL planner.
    pub name: &'static str,
    /// SQL spellings which resolve to `name`.
    pub aliases: &'static [&'static str],
    /// Inclusive minimum and optional maximum number of expression arguments.
    /// A wildcard argument is described separately.
    pub arity: SqlFunctionArity,
    /// Constraint on each expression argument accepted by this aggregate.
    pub input: SqlAggregateInput,
    /// Whether `DISTINCT` is supported and, when needed, its argument limit.
    pub distinct: SqlAggregateDistinct,
    /// Whether the SQL wildcard argument is accepted, and under which form.
    pub wildcard: SqlAggregateWildcard,
    /// Engine-independent output type behavior. The `nullable` flag describes
    /// the SQL result for an empty/all-null input group.
    pub output: SqlAggregateOutput,
}

/// Arity constraint for a function's expression arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqlFunctionArity {
    pub min: u8,
    pub max: Option<u8>,
}

impl SqlFunctionArity {
    pub const fn exact(count: u8) -> Self {
        Self {
            min: count,
            max: Some(count),
        }
    }

    pub const fn at_least(min: u8) -> Self {
        Self { min, max: None }
    }

    pub const fn accepts(self, count: usize) -> bool {
        if count < self.min as usize {
            return false;
        }
        match self.max {
            Some(max) => count <= max as usize,
            None => true,
        }
    }
}

/// Data type constraint for an aggregate's expression arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlAggregateInput {
    /// Any scalar expression type; NULL values are handled by the aggregate.
    AnyScalar,
    /// Numeric values supported by Ugoite's checked numeric accumulators.
    Numeric,
    /// A type for which DataFusion provides ordered MIN/MAX accumulation.
    Orderable,
}

/// DISTINCT behavior for an aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlAggregateDistinct {
    Unsupported,
    /// DISTINCT accepts exactly one expression argument.
    SingleArgument,
}

/// Wildcard behavior for an aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlAggregateWildcard {
    Unsupported,
    /// Accept `(*)` both with and without DISTINCT.
    WithOrWithoutDistinct,
}

/// Result type behavior for an aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqlAggregateOutput {
    pub data_type: SqlAggregateOutputType,
    pub nullable: bool,
}

/// A stable description of how the result type follows from aggregate inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlAggregateOutputType {
    /// Signed 64-bit integer, independent of argument type.
    Int64,
    /// The input's numeric type promoted using Ugoite's checked SUM rules.
    NumericSum,
    /// The input's numeric type promoted using Ugoite's checked AVG rules.
    NumericAverage,
    /// The input expression's type, unchanged.
    Input,
}

const COUNT: SqlAggregateFunction = SqlAggregateFunction {
    name: "count",
    aliases: &[],
    // DataFusion accepts COUNT(), COUNT(*), and COUNT(expr, ...). The wildcard
    // spelling is represented separately from expression arity.
    arity: SqlFunctionArity::at_least(0),
    input: SqlAggregateInput::AnyScalar,
    distinct: SqlAggregateDistinct::SingleArgument,
    wildcard: SqlAggregateWildcard::WithOrWithoutDistinct,
    output: SqlAggregateOutput {
        data_type: SqlAggregateOutputType::Int64,
        nullable: false,
    },
};

const SUM: SqlAggregateFunction = SqlAggregateFunction {
    name: "sum",
    aliases: &[],
    arity: SqlFunctionArity::exact(1),
    input: SqlAggregateInput::Numeric,
    distinct: SqlAggregateDistinct::SingleArgument,
    wildcard: SqlAggregateWildcard::Unsupported,
    output: SqlAggregateOutput {
        data_type: SqlAggregateOutputType::NumericSum,
        nullable: true,
    },
};

const AVG: SqlAggregateFunction = SqlAggregateFunction {
    name: "avg",
    aliases: &["mean"],
    arity: SqlFunctionArity::exact(1),
    input: SqlAggregateInput::Numeric,
    distinct: SqlAggregateDistinct::SingleArgument,
    wildcard: SqlAggregateWildcard::Unsupported,
    output: SqlAggregateOutput {
        data_type: SqlAggregateOutputType::NumericAverage,
        nullable: true,
    },
};

const MIN: SqlAggregateFunction = SqlAggregateFunction {
    name: "min",
    aliases: &[],
    arity: SqlFunctionArity::exact(1),
    input: SqlAggregateInput::Orderable,
    distinct: SqlAggregateDistinct::SingleArgument,
    wildcard: SqlAggregateWildcard::Unsupported,
    output: SqlAggregateOutput {
        data_type: SqlAggregateOutputType::Input,
        nullable: true,
    },
};

const MAX: SqlAggregateFunction = SqlAggregateFunction {
    name: "max",
    aliases: &[],
    arity: SqlFunctionArity::exact(1),
    input: SqlAggregateInput::Orderable,
    distinct: SqlAggregateDistinct::SingleArgument,
    wildcard: SqlAggregateWildcard::Unsupported,
    output: SqlAggregateOutput {
        data_type: SqlAggregateOutputType::Input,
        nullable: true,
    },
};

/// The closed aggregate set admitted for user SQL by
/// `SqlFunctionPolicy::ReadOnlyAggregateV1`.
pub const USER_SQL_AGGREGATES: &[SqlAggregateFunction] = &[COUNT, SUM, AVG, MIN, MAX];

/// Finds an admitted user-SQL aggregate by canonical name or alias,
/// case-insensitively.
pub fn user_sql_aggregate(name: &str) -> Option<&'static SqlAggregateFunction> {
    USER_SQL_AGGREGATES.iter().find(|function| {
        function.name.eq_ignore_ascii_case(name)
            || function
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        user_sql_aggregate, SqlAggregateDistinct, SqlAggregateInput, SqlAggregateOutputType,
        SqlAggregateWildcard, SqlFunctionArity, USER_SQL_AGGREGATES,
    };

    #[test]
    fn user_sql_aggregate_registry_describes_the_admitted_set() {
        assert_eq!(
            USER_SQL_AGGREGATES
                .iter()
                .map(|function| function.name)
                .collect::<Vec<_>>(),
            ["count", "sum", "avg", "min", "max"]
        );

        let count = user_sql_aggregate("COUNT").expect("COUNT descriptor");
        assert_eq!(count.arity, SqlFunctionArity::at_least(0));
        assert!(count.arity.accepts(0));
        assert!(count.arity.accepts(5));
        assert_eq!(count.input, SqlAggregateInput::AnyScalar);
        assert_eq!(count.distinct, SqlAggregateDistinct::SingleArgument);
        assert_eq!(count.wildcard, SqlAggregateWildcard::WithOrWithoutDistinct);
        assert_eq!(count.output.data_type, SqlAggregateOutputType::Int64);
        assert!(!count.output.nullable);

        let sum = user_sql_aggregate("sum").expect("SUM descriptor");
        assert_eq!(sum.arity, SqlFunctionArity::exact(1));
        assert_eq!(sum.input, SqlAggregateInput::Numeric);
        assert_eq!(sum.distinct, SqlAggregateDistinct::SingleArgument);
        assert_eq!(sum.wildcard, SqlAggregateWildcard::Unsupported);
        assert_eq!(sum.output.data_type, SqlAggregateOutputType::NumericSum);
        assert!(sum.output.nullable);

        let avg = user_sql_aggregate("avg").expect("AVG descriptor");
        assert_eq!(avg.aliases, ["mean"]);
        assert_eq!(avg.input, SqlAggregateInput::Numeric);
        assert_eq!(avg.output.data_type, SqlAggregateOutputType::NumericAverage);
        assert!(avg.output.nullable);
        assert_eq!(user_sql_aggregate("MEAN"), Some(avg));

        for name in ["min", "max"] {
            let function = user_sql_aggregate(name).expect("ordered aggregate descriptor");
            assert_eq!(function.arity, SqlFunctionArity::exact(1));
            assert_eq!(function.input, SqlAggregateInput::Orderable);
            assert_eq!(function.distinct, SqlAggregateDistinct::SingleArgument);
            assert_eq!(function.wildcard, SqlAggregateWildcard::Unsupported);
            assert_eq!(function.output.data_type, SqlAggregateOutputType::Input);
            assert!(function.output.nullable);
        }
    }

    #[test]
    fn aggregate_lookup_is_closed_and_case_insensitive() {
        assert!(user_sql_aggregate("count").is_some());
        assert!(user_sql_aggregate("MeAn").is_some());
        for name in ["lower", "row_number", "approx_count_distinct", "median"] {
            assert!(
                user_sql_aggregate(name).is_none(),
                "unexpected aggregate {name}"
            );
        }
    }
}
