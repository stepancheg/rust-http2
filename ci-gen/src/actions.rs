use crate::ghwf::Step;
use crate::yaml::Yaml;

pub fn checkout_sources() -> Step {
    Step::uses("Checkout sources", "actions/checkout@v2")
}

pub fn cargo(name: &str, command: &str, args: &str) -> Step {
    let mut with = vec![("command", command)];
    if !args.is_empty() {
        with.push(("args", args));
    }
    Step::uses_with(
        name,
        "actions-rs/cargo@v1",
        Yaml::map(with),
    )
}

pub fn cargo_test(name: &str, args: &str) -> Step {
    cargo(name, "test", args)
}

pub fn cargo_build(name: &str, args: &str) -> Step {
    cargo(name, "build", args)
}

pub fn cargo_doc(name: &str, args: &str) -> Step {
    cargo(name, "doc", args)
}
