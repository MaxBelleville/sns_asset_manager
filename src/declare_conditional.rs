use crate::SymbolMap;
use crate::asset_shared::{AssetGroup, AssetId, AssetIdVal};
use crate::registry::{AssetRegistration, Registry};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Marker alias for a boolean field resolved from a Declare(Conditional) reference.
pub type Conditional = bool;

/// RON expression tree stored by a `Declare(Conditional)` asset.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConditionalExpr {
    #[serde(default)]
    pub and: Vec<ConditionalExpr>,
    #[serde(default)]
    pub or: Vec<ConditionalExpr>,
    #[serde(default)]
    pub not: Vec<ConditionalExpr>,
    #[serde(default)]
    pub cmp_left: ConditionalOperand,
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub cmp_right: ConditionalOperand,
}

impl Default for ConditionalExpr {
    fn default() -> Self {
        Self {
            and: Vec::new(),
            or: Vec::new(),
            not: Vec::new(),
            cmp_left: ConditionalOperand::default(),
            op: String::new(),
            cmp_right: ConditionalOperand::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConditionalOperand {
    #[serde(default = "unit_value")]
    pub field: ron::Value,
    #[serde(default = "unit_value")]
    pub value: ron::Value,
}

fn unit_value() -> ron::Value {
    ron::Value::Unit
}

impl Default for ConditionalOperand {
    fn default() -> Self {
        Self {
            field: ron::Value::Unit,
            value: ron::Value::Unit,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ComparisonOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclareConditionalRaw {
    pub expr: ConditionalExpr,
}

#[derive(Debug, Clone)]
pub struct DeclareConditional {
    expr: ConditionalExpr,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[doc(hidden)]
pub struct ConditionalRefRaw(pub AssetId);

impl AssetGroup for DeclareConditional {
    const STRUCT_NAME: &'static str = "DeclareConditional";
    type Raw = DeclareConditionalRaw;
    type View = bool;

    fn from_raw(
        raw: &Self::Raw,
        _registry: &mut Registry,
        _symbols: &SymbolMap,
        _asset_id_string: String,
    ) -> Result<Self, String> {
        Ok(Self {
            expr: raw.expr.clone(),
        })
    }
}

#[derive(Debug, Clone)]
enum Scalar {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    EnumFlags {
        selected: Vec<String>,
        allowed: Vec<String>,
    },
}

impl Scalar {
    fn from_ron(value: ron::Value, reference: &str) -> Result<Self, String> {
        match value {
            ron::Value::Bool(v) => Ok(Self::Bool(v)),
            ron::Value::String(v) => Ok(Self::String(v)),
            ron::Value::Number(ron::Number::F32(v)) => Ok(Self::Float(v.0 as f64)),
            ron::Value::Number(ron::Number::F64(v)) => Ok(Self::Float(v.0)),
            ron::Value::Number(number) => {
                let integer = match number {
                    ron::Number::I8(v) => Some(i64::from(v)),
                    ron::Number::I16(v) => Some(i64::from(v)),
                    ron::Number::I32(v) => Some(i64::from(v)),
                    ron::Number::I64(v) => Some(v),
                    ron::Number::U8(v) => Some(i64::from(v)),
                    ron::Number::U16(v) => Some(i64::from(v)),
                    ron::Number::U32(v) => Some(i64::from(v)),
                    ron::Number::U64(v) => i64::try_from(v).ok(),
                    _ => None,
                };
                integer.map(Self::Integer).ok_or_else(|| {
                    format!("Conditional field '{reference}' is not a supported integer")
                })
            }
            _ => Err(format!(
                "Conditional field '{reference}' is not a supported scalar"
            )),
        }
    }

    fn compare(&self, op: ComparisonOp, rhs: &Self) -> Result<bool, String> {
        match (self, rhs) {
            (Self::EnumFlags { selected, allowed }, Self::String(item))
            | (Self::String(item), Self::EnumFlags { selected, allowed }) => {
                if !allowed.iter().any(|candidate| candidate == item) {
                    return Err(format!(
                        "Unknown enum item '{item}' in conditional comparison"
                    ));
                }
                return match op {
                    ComparisonOp::Eq => Ok(selected.iter().any(|value| value == item)),
                    ComparisonOp::Ne => Ok(!selected.iter().any(|value| value == item)),
                    _ => Err("EnumFlags support only Eq and Ne against a string item".to_owned()),
                };
            }
            (Self::EnumFlags { .. }, _) | (_, Self::EnumFlags { .. }) => {
                return Err("EnumFlags can only be compared with a string item".to_owned());
            }
            _ => {}
        }
        if matches!(
            (self, rhs, op),
            (
                Self::Bool(_),
                Self::Bool(_),
                ComparisonOp::Lt | ComparisonOp::Le | ComparisonOp::Gt | ComparisonOp::Ge
            )
        ) {
            return Err("Boolean condition values support only Eq and Ne".to_owned());
        }
        let ordering = match (self, rhs) {
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a
                .partial_cmp(b)
                .ok_or_else(|| "Conditional cannot compare NaN values".to_owned())?,
            (Self::String(a), Self::String(b)) => a.cmp(b),
            _ => return Err("Conditional comparison operands have incompatible types".to_owned()),
        };
        match op {
            ComparisonOp::Eq => Ok(ordering == Ordering::Equal),
            ComparisonOp::Ne => Ok(ordering != Ordering::Equal),
            ComparisonOp::Lt => Ok(ordering == Ordering::Less),
            ComparisonOp::Le => Ok(ordering != Ordering::Greater),
            ComparisonOp::Gt => Ok(ordering == Ordering::Greater),
            ComparisonOp::Ge => Ok(ordering != Ordering::Less),
        }
    }
}

impl ConditionalOperand {
    fn resolve(&self, registry: &Registry, symbols: &SymbolMap) -> Result<Scalar, String> {
        match (&self.field, &self.value) {
            (ron::Value::String(raw_reference), ron::Value::Unit) => {
                let reference = AssetId::parse(raw_reference)?;
                let field = reference.field().ok_or_else(|| {
                    format!("Conditional reference '{reference}' must select a field")
                })?;
                let id = reference.get();
                if !symbols.contains_key(&id) {
                    return Err(format!(
                        "Conditional asset reference '{reference}' does not exist"
                    ));
                }
                if !registry.is_static_raw(&id) {
                    return Err(format!(
                        "Conditional field reference '{reference}' must target a static asset"
                    ));
                }
                let raw = registry
                    .get_erased_raw(&id)
                    .ok_or_else(|| format!("Conditional asset '{reference}' is not loaded"))?;
                let getter = registry
                    .raw_field_getter(raw.as_ref().type_id())
                    .ok_or_else(|| {
                        format!(
                            "No field parser is registered for conditional target '{reference}'"
                        )
                    })?;
                let value = getter(raw, field).map_err(|error| {
                    format!("Failed to read conditional field '{reference}': {error}")
                })?;
                match value {
                    ron::Value::Map(_) => {
                        let (selected, allowed) = crate::declare_enum::enum_flags_from_ron(
                            value, registry,
                        )
                        .map_err(|error| {
                            format!("Failed to resolve EnumFlags field '{reference}': {error}")
                        })?;
                        Ok(Scalar::EnumFlags { selected, allowed })
                    }
                    value => Scalar::from_ron(value, &reference.to_string()),
                }
            }
            (ron::Value::Unit, value) if !matches!(value, ron::Value::Unit) => {
                Scalar::from_ron(value.clone(), "<literal>")
            }
            (ron::Value::Unit, ron::Value::Unit) => {
                Err("Conditional operand requires 'field' or 'value'".to_owned())
            }
            _ => {
                Err("Conditional operand must contain exactly one of 'field' or 'value'".to_owned())
            }
        }
    }
}

impl ConditionalExpr {
    pub(crate) fn evaluate(
        &self,
        registry: &Registry,
        symbols: &SymbolMap,
    ) -> Result<bool, String> {
        let compare_present = !self.op.is_empty()
            || !matches!(&self.cmp_left.field, &ron::Value::Unit)
            || !matches!(&self.cmp_left.value, &ron::Value::Unit)
            || !matches!(&self.cmp_right.field, &ron::Value::Unit)
            || !matches!(&self.cmp_right.value, &ron::Value::Unit);
        let branch_count = usize::from(!self.and.is_empty())
            + usize::from(!self.or.is_empty())
            + usize::from(!self.not.is_empty())
            + usize::from(compare_present);
        if branch_count != 1 {
            return Err(
                "Conditional expression must specify exactly one of and, or, not, or comparison"
                    .to_owned(),
            );
        }

        if !self.and.is_empty() {
            let mut result = true;
            for item in &self.and {
                result &= item.evaluate(registry, symbols)?;
            }
            Ok(result)
        } else if !self.or.is_empty() {
            let mut result = false;
            for item in &self.or {
                result |= item.evaluate(registry, symbols)?;
            }
            Ok(result)
        } else if !self.not.is_empty() {
            if self.not.len() != 1 {
                return Err("Conditional Not requires exactly one item".to_owned());
            }
            Ok(!self.not[0].evaluate(registry, symbols)?)
        } else {
            let op = match self.op.as_str() {
                "eq" => ComparisonOp::Eq,
                "ne" => ComparisonOp::Ne,
                "lt" => ComparisonOp::Lt,
                "le" => ComparisonOp::Le,
                "gt" => ComparisonOp::Gt,
                "ge" => ComparisonOp::Ge,
                value => {
                    return Err(format!(
                        "Unsupported conditional comparison operator '{value}'"
                    ));
                }
            };
            self.cmp_left
                .resolve(registry, symbols)?
                .compare(op, &self.cmp_right.resolve(registry, symbols)?)
        }
    }
}

impl AssetIdVal<DeclareConditional> {
    #[doc(hidden)]
    pub fn conditional_value(
        &self,
        registry: &Registry,
        symbols: &SymbolMap,
    ) -> Result<bool, String> {
        registry
            .lookup_asset_by_id::<DeclareConditional>(self.get())
            .ok_or_else(|| format!("Declare(Conditional) ID {} is not loaded", self.get()))?
            .expr
            .evaluate(registry, symbols)
    }
}

pub(crate) fn validate_loaded(registry: &Registry, symbols: &SymbolMap) -> Result<(), String> {
    for conditional in registry.get_all::<DeclareConditional>() {
        conditional.expr.evaluate(registry, symbols)?;
    }
    Ok(())
}

inventory::submit! {
    AssetRegistration {
        register_fn: |registry| registry.__register::<DeclareConditional>(),
    }
}
