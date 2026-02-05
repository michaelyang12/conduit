/// Structured system instructions for LLM requests.
///
/// Use the builder to compose instructions, or pass a raw string.
pub struct Instructions {
    content: String,
}

impl Instructions {
    pub fn builder() -> InstructionsBuilder {
        InstructionsBuilder::default()
    }

    pub fn as_str(&self) -> &str {
        &self.content
    }
}

impl From<&str> for Instructions {
    fn from(s: &str) -> Self {
        Instructions {
            content: s.to_string(),
        }
    }
}

impl From<String> for Instructions {
    fn from(s: String) -> Self {
        Instructions { content: s }
    }
}

#[derive(Default)]
pub struct InstructionsBuilder {
    role: Option<String>,
    output_format: Option<String>,
    modes: Vec<(String, String)>,
    constraints: Vec<String>,
}

impl InstructionsBuilder {
    pub fn role(mut self, role: &str) -> Self {
        self.role = Some(role.to_string());
        self
    }

    pub fn output_format(mut self, format: &str) -> Self {
        self.output_format = Some(format.to_string());
        self
    }

    pub fn mode(mut self, name: &str, description: &str) -> Self {
        self.modes.push((name.to_string(), description.to_string()));
        self
    }

    pub fn constraint(mut self, constraint: &str) -> Self {
        self.constraints.push(constraint.to_string());
        self
    }

    pub fn build(self) -> Instructions {
        let mut xml = String::from("<system_instructions>\n");

        if let Some(role) = &self.role {
            xml.push_str(&format!("  <role>{}</role>\n", role));
        }

        if let Some(fmt) = &self.output_format {
            xml.push_str(&format!("  <output_format>{}</output_format>\n", fmt));
        }

        if !self.modes.is_empty() {
            xml.push_str("  <modes>\n");
            for (name, desc) in &self.modes {
                xml.push_str(&format!("    <mode name=\"{}\">{}</mode>\n", name, desc));
            }
            xml.push_str("  </modes>\n");
        }

        if !self.constraints.is_empty() {
            xml.push_str("  <constraints>\n");
            for c in &self.constraints {
                xml.push_str(&format!("    <constraint>{}</constraint>\n", c));
            }
            xml.push_str("  </constraints>\n");
        }

        xml.push_str("</system_instructions>");

        Instructions { content: xml }
    }
}
