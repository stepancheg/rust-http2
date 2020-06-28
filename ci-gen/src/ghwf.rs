use crate::yaml::Yaml;

/// Github workflow step
pub struct Step(pub Yaml);

impl Step {
    pub fn uses(name: &str, uses: &str) -> Step {
        Step(Yaml::map(vec![("name", name), ("uses", uses)]))
    }

    pub fn uses_with(name: &str, uses: &str, with: Yaml) -> Step {
        Step(Yaml::map(vec![
            ("name", Yaml::string(name)),
            ("uses", Yaml::string(uses)),
            ("with", with),
        ]))
    }

    pub fn run(name: &str, run: &str) -> Step {
        Step(Yaml::map(vec![("name", name), ("run", run)]))
    }
}

impl Into<Yaml> for Step {
    fn into(self) -> Yaml {
        self.0
    }
}

pub struct Job {
    pub id: String,
    pub name: String,
    pub runs_on: String,
    pub steps: Vec<Step>,
}

impl Into<(String, Yaml)> for Job {
    fn into(self) -> (String, Yaml) {
        (
            self.id,
            Yaml::map(vec![
                ("name", Yaml::string(self.name)),
                ("runs-on", Yaml::string(self.runs_on)),
                ("steps", Yaml::list(self.steps)),
            ]),
        )
    }
}
