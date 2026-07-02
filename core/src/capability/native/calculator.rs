use std::sync::LazyLock;

use log::error;
use regex::Regex;
use wl_clipboard_rs::copy::{MimeType, Options, Source};

use crate::capability::{
    Action, ActionOutcome, Capability, CapabilityMeta, IconRef, Item, QueryHandler,
};

const MAX_EXPRESSION_BYTES: usize = 1024;
const ICON_NAME: &str = "accessories-calculator";
const COPY_RESULT_ACTION_LABEL: &str = "Copy Result";
const COPY_EQUATION_ACTION_LABEL: &str = "Copy Equation";
const TRIGGER_PREFIX: char = '=';

/// exmex only knows the constants `PI`/`π`, but launcher users type `pi`.
static PI_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bpi\b").expect("valid regex"));

pub struct Calculator;

impl Capability for Calculator {
    fn id(&self) -> &'static str {
        "calculator"
    }

    fn metadata(&self) -> CapabilityMeta {
        CapabilityMeta {
            name: "Calculator".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Evaluate mathematical expressions.".to_string(),
            icon: None,
            homepage: None,
            author: None,
        }
    }
}

impl QueryHandler for Calculator {
    fn query(&self, input: &str) -> Vec<Item> {
        evaluate(input)
            .map(|(expression, value)| build_item(expression, value))
            .into_iter()
            .collect()
    }

    fn run(&self, action: Action) -> ActionOutcome {
        let Some(text) = action.params.into_iter().next() else {
            error!("calculator: action with no payload");
            return ActionOutcome::Hide;
        };

        match action.label.as_str() {
            COPY_RESULT_ACTION_LABEL | COPY_EQUATION_ACTION_LABEL => copy_to_clipboard(&text),
            other => error!("calculator: unknown action label: {other}"),
        }

        ActionOutcome::Hide
    }
}

fn evaluate(input: &str) -> Option<(&str, String)> {
    let trimmed = input.trim();
    let (expression, triggered) = match trimmed.strip_prefix(TRIGGER_PREFIX) {
        Some(rest) => (rest.trim(), true),
        None => (trimmed, false),
    };

    if expression.is_empty() || expression.len() > MAX_EXPRESSION_BYTES {
        return None;
    }

    // without the `=` trigger, only input that looks like a computation is
    // evaluated: it needs a digit and must not be a bare number echoing back
    if !triggered
        && (!expression.bytes().any(|b| b.is_ascii_digit()) || expression.parse::<f64>().is_ok())
    {
        return None;
    }

    let normalized = PI_WORD.replace_all(expression, "π");
    let value = exmex::eval_str::<f64>(&normalized).ok()?;
    value.is_finite().then(|| (expression, format_value(value)))
}

/// round to 10 decimals to hide float artifacts (0.1 + 0.2 -> 0.3)
fn format_value(value: f64) -> String {
    const SCALE: f64 = 1e10;
    let rounded = (value * SCALE).round() / SCALE;
    if rounded.is_finite() {
        rounded.to_string()
    } else {
        value.to_string()
    }
}

fn build_item(expression: &str, value: String) -> Item {
    let equation = format!("{expression} = {value}");
    Item {
        title: equation.clone(),
        icon: Some(IconRef::Name(ICON_NAME.into())),
        actions: vec![
            Action {
                label: COPY_RESULT_ACTION_LABEL.into(),
                params: vec![value],
                primary: true,
            },
            Action {
                label: COPY_EQUATION_ACTION_LABEL.into(),
                params: vec![equation],
                primary: false,
            },
        ],
    }
}

fn copy_to_clipboard(text: &str) {
    let opts = Options::new();
    if let Err(e) = opts.copy(Source::Bytes(text.as_bytes().into()), MimeType::Autodetect) {
        error!("calculator: copy failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_arithmetic_expression() {
        assert_eq!(evaluate("1 + 2 * 3"), Some(("1 + 2 * 3", "7".to_string())));
    }

    #[test]
    fn evaluates_functions_and_constants() {
        assert_eq!(
            evaluate("sin(pi / 2) + sqrt(9)"),
            Some(("sin(pi / 2) + sqrt(9)", "4".to_string()))
        );
    }

    #[test]
    fn rounds_float_artifacts() {
        assert_eq!(
            evaluate("0.1 + 0.2"),
            Some(("0.1 + 0.2", "0.3".to_string()))
        );
    }

    #[test]
    fn rejects_empty_expression() {
        assert_eq!(evaluate("   "), None);
    }

    #[test]
    fn rejects_too_long_expression() {
        assert_eq!(evaluate(&"1+1".repeat(MAX_EXPRESSION_BYTES)), None);
    }

    #[test]
    fn rejects_non_math_input() {
        assert_eq!(evaluate("firefox 2"), None);
    }

    #[test]
    fn rejects_bare_number() {
        assert_eq!(evaluate("2"), None);
        assert_eq!(evaluate("-2.5"), None);
    }

    #[test]
    fn rejects_input_without_digits() {
        assert_eq!(evaluate("pi"), None);
    }

    #[test]
    fn trigger_evaluates_constants() {
        assert_eq!(evaluate("= pi"), Some(("pi", "3.1415926536".to_string())));
    }

    #[test]
    fn query_returns_item_with_copy_actions() {
        let items = Calculator.query("2 + 2");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "2 + 2 = 4");
        assert_eq!(items[0].actions.len(), 2);
        assert_eq!(items[0].actions[0].label, COPY_RESULT_ACTION_LABEL);
        assert_eq!(items[0].actions[0].params, vec!["4".to_string()]);
        assert!(items[0].actions[0].primary);
        assert_eq!(items[0].actions[1].label, COPY_EQUATION_ACTION_LABEL);
        assert_eq!(items[0].actions[1].params, vec!["2 + 2 = 4".to_string()]);
        assert!(!items[0].actions[1].primary);
    }
}
