#[derive(Clone)]
pub enum Yaml {
    String(String),
    Int(i64),
    List(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
    Bool(bool),
}

impl Yaml {
    pub fn to_yaml_rust(&self) -> yaml_rust::Yaml {
        match self {
            Yaml::String(s) => yaml_rust::Yaml::String(s.clone()),
            Yaml::List(l) => yaml_rust::Yaml::Array(l.iter().map(|i| i.to_yaml_rust()).collect()),
            Yaml::Map(m) => yaml_rust::Yaml::Hash(
                m.iter()
                    .map(|(k, v)| (yaml_rust::Yaml::String(k.clone()), v.to_yaml_rust()))
                    .collect(),
            ),
            Yaml::Int(i) => yaml_rust::Yaml::Integer(*i),
            Yaml::Bool(b) => yaml_rust::Yaml::Boolean(*b),
        }
    }
}

impl From<&Yaml> for Yaml {
    fn from(y: &Yaml) -> Self {
        y.clone()
    }
}

impl From<String> for Yaml {
    fn from(s: String) -> Self {
        Yaml::String(s)
    }
}

impl From<&str> for Yaml {
    fn from(s: &str) -> Self {
        Yaml::String(s.to_owned())
    }
}

impl From<&&str> for Yaml {
    fn from(s: &&str) -> Self {
        Yaml::String((*s).to_owned())
    }
}

impl From<bool> for Yaml {
    fn from(b: bool) -> Self {
        Yaml::Bool(b)
    }
}

impl<T: Into<Yaml>> From<Vec<T>> for Yaml {
    fn from(v: Vec<T>) -> Self {
        Yaml::List(v.into_iter().map(|t| t.into()).collect())
    }
}

impl Yaml {
    pub fn map<K: Into<String>, V: Into<Yaml>, E: IntoIterator<Item = (K, V)>>(entries: E) -> Yaml {
        Yaml::Map(
            entries
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }

    pub fn list<V: Into<Yaml>, E: IntoIterator<Item = V>>(values: E) -> Yaml {
        Yaml::List(values.into_iter().map(|v| v.into()).collect())
    }

    pub fn string<S: Into<String>>(s: S) -> Yaml {
        Yaml::String(s.into())
    }
}

#[derive(Default)]
pub struct YamlWriter {
    pub buffer: String,
}

impl YamlWriter {
    pub fn write_line(&mut self, line: &str) {
        self.buffer.push_str(line);
        self.buffer.push_str("\n");
    }

    pub fn write_yaml(&mut self, yaml: &Yaml) {
        yaml_rust::emitter::YamlEmitter::new(&mut self.buffer)
            .compact()
            .dump(&yaml.to_yaml_rust())
            .unwrap();
    }
}
