use crate::yaml::Yaml;
use std::fmt;

#[allow(dead_code)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Env {
    WindowsLatest,
    UbuntuLatest,
    MacosLatest,
}

impl fmt::Display for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Env::WindowsLatest => write!(f, "windows-latest"),
            Env::UbuntuLatest => write!(f, "ubuntu-latest"),
            Env::MacosLatest => write!(f, "macos-latest"),
        }
    }
}

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
    pub runs_on: Env,
    pub steps: Vec<Step>,
}

impl Into<(String, Yaml)> for Job {
    fn into(self) -> (String, Yaml) {
        (
            self.id,
            Yaml::map(vec![
                ("name", Yaml::string(self.name)),
                ("runs-on", Yaml::string(format!("{}", self.runs_on))),
                ("steps", Yaml::list(self.steps)),
            ]),
        )
    }
}
