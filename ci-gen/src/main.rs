use gh_actions_gen::actions::cargo_build;
use gh_actions_gen::actions::cargo_cache;
use gh_actions_gen::actions::cargo_doc;
use gh_actions_gen::actions::cargo_test;
use gh_actions_gen::actions::checkout_sources;
use gh_actions_gen::actions::rust_install_toolchain;
use gh_actions_gen::actions::RustToolchain;
use gh_actions_gen::ghwf::Env;
use gh_actions_gen::ghwf::Job;
use gh_actions_gen::ghwf::Step;

#[derive(PartialEq, Eq, Copy, Clone)]
struct Os {
    name: &'static str,
    ghwf: Env,
}

const LINUX: Os = Os {
    name: "linux",
    ghwf: Env::UbuntuLatest,
};
const MACOS: Os = Os {
    name: "macos",
    ghwf: Env::MacosLatest,
};
const WINDOWS: Os = Os {
    name: "windows",
    ghwf: Env::WindowsLatest,
};

fn steps(os: Os, channel: RustToolchain) -> Vec<Step> {
    let mut r = Vec::new();
    r.push(cargo_cache());
    r.push(checkout_sources());
    r.push(rust_install_toolchain(channel));

    // Slow as death
    if false && os.ghwf == Env::WindowsLatest {
        r.push(Step::run(
            "Install OpenSSL",
            "vcpkg install openssl:x64-windows",
        ));
    }

    if os.ghwf == Env::WindowsLatest {
        // It's too hard to install OpenSSL on Windows, so build main crate only

        r.push(cargo_build("cargo build main crate only", "-p httpbis"));
    } else {
        // Use one thread for better errors
        r.push(
            cargo_test(
                &format!("cargo test"),
                "--all --all-targets -- --test-threads=1",
            )
            .with_timeout_minutes(10),
        );
        // `--all-targets` does not include doctests
        // https://github.com/rust-lang/cargo/issues/6669
        r.push(cargo_test("cargo test --doc", "--doc"));

        // No need to waste time generating docs everywhere
        if os.ghwf == Env::UbuntuLatest {
            r.push(cargo_doc("doc", ""));
        }
    }
    r
}

fn h2spec_test_job() -> Job {
    Job {
        id: format!("h2spec"),
        name: format!("h2spec"),
        runs_on: LINUX.ghwf.to_owned(),
        steps: vec![
            cargo_cache(),
            checkout_sources(),
            Step::run("Install h2spec", "ci/install-h2spec.sh"),
            cargo_build("Build h2spec test", "--manifest-path h2spec-test/Cargo.toml --bin the_test"),
            Step::run(
                "Run h2spec test",
                "PATH=\"$(pwd):$PATH\" cargo run --manifest-path h2spec-test/Cargo.toml --bin the_test",
            ).with_timeout_minutes(5)
        ],
        ..Default::default()
    }
}

fn jobs() -> Vec<Job> {
    let mut r = Vec::new();
    for &channel in &[
        RustToolchain::Stable,
        RustToolchain::Beta,
        RustToolchain::Nightly,
    ] {
        for &os in &[LINUX, MACOS, WINDOWS] {
            if channel != RustToolchain::Stable && os != LINUX {
                // skip some jobs because macos and windows are expensive
                continue;
            }
            r.push(Job {
                id: format!("{}-{}", os.name, channel),
                name: format!("{} {}", os.name, channel),
                runs_on: os.ghwf.to_owned(),
                steps: steps(os, channel),
                env: if os == WINDOWS {
                    vec![("VCPKGRS_DYNAMIC".to_owned(), "1".to_owned())]
                } else {
                    Vec::new()
                },
                ..Default::default()
            });
        }
    }

    r.push(h2spec_test_job());

    r
}

fn main() {
    gh_actions_gen::write(jobs());
}
