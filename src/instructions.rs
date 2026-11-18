/// A system prompt. Build one with [`Instructions::builder`] or convert from any string.
///
/// The builder renders tagged sections in a fixed order (role, custom sections, output format,
/// rules, examples) so the format and rules sit closest to the input, where models weigh them most.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions(String);

impl Instructions {
    pub fn builder() -> InstructionsBuilder {
        InstructionsBuilder::default()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Instructions {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for Instructions {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&String> for Instructions {
    fn from(s: &String) -> Self {
        Self(s.clone())
    }
}

impl From<&Instructions> for Instructions {
    fn from(i: &Instructions) -> Self {
        i.clone()
    }
}

#[derive(Debug, Default, Clone)]
pub struct InstructionsBuilder {
    role: Option<String>,
    sections: Vec<(String, String)>,
    format: Option<String>,
    rules: Vec<String>,
    examples: Vec<(String, String)>,
}

impl InstructionsBuilder {
    /// Who the model is and what it does, e.g. "You convert natural language into shell commands."
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    /// Any extra block of guidance, rendered as `<name>body</name>`.
    pub fn section(mut self, name: impl Into<String>, body: impl Into<String>) -> Self {
        self.sections.push((name.into(), body.into()));
        self
    }

    /// The exact shape of the answer.
    pub fn format(mut self, format: impl Into<String>) -> Self {
        self.format = Some(format.into());
        self
    }

    /// A hard constraint. Rules render as a list.
    pub fn rule(mut self, rule: impl Into<String>) -> Self {
        self.rules.push(rule.into());
        self
    }

    /// An input/output pair showing a correct answer. Small local models benefit the most.
    pub fn example(mut self, input: impl Into<String>, output: impl Into<String>) -> Self {
        self.examples.push((input.into(), output.into()));
        self
    }

    pub fn build(self) -> Instructions {
        let mut blocks = Vec::new();
        if let Some(role) = self.role {
            blocks.push(tag("role", &role));
        }
        for (name, body) in &self.sections {
            blocks.push(tag(name, body));
        }
        if let Some(format) = self.format {
            blocks.push(tag("output_format", &format));
        }
        if !self.rules.is_empty() {
            let list: Vec<String> = self.rules.iter().map(|r| format!("- {r}")).collect();
            blocks.push(tag("rules", &list.join("\n")));
        }
        if !self.examples.is_empty() {
            let examples: Vec<String> = self
                .examples
                .iter()
                .map(|(i, o)| {
                    tag(
                        "example",
                        &format!("{}\n{}", tag("input", i), tag("output", o)),
                    )
                })
                .collect();
            blocks.push(tag("examples", &examples.join("\n")));
        }
        Instructions(blocks.join("\n\n"))
    }
}

pub(crate) fn tag(name: &str, body: &str) -> String {
    format!("<{name}>\n{}\n</{name}>", body.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sections_in_a_stable_order() {
        let built = Instructions::builder()
            .rule("never explain")
            .format("one line")
            .example("list files", "ls")
            .section("safety", "confirm destructive operations")
            .role("You write shell commands.")
            .build();
        let text = built.as_str();
        let order = [
            "<role>",
            "<safety>",
            "<output_format>",
            "<rules>",
            "<examples>",
        ];
        let positions: Vec<usize> = order.iter().map(|t| text.find(t).unwrap()).collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(text.contains("- never explain"));
        assert!(text.contains("<input>\nlist files\n</input>"));
    }
}
