//! The browser half of a room, and the build step that compiles it.
//!
//! Browsers run JavaScript and nothing else, so something has to produce
//! it. Nothing in these repos is written in it: the source is TypeScript,
//! and this turns that into one file while the crate is being built. A tool
//! calls [`bundle`] from its `build.rs` and includes what comes back.
//!
//! ```ignore
//! // build.rs
//! rooms_client::bundle("client");
//! ```
//!
//! ```ignore
//! // src/main.rs
//! const APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.js"));
//! const APP_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/app.css"));
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The TypeScript every one of these tools shares.
pub const ROOMS_TS: &str = include_str!("../client/rooms.ts");

/// The stylesheet every one of these tools shares.
pub const ROOMS_CSS: &str = include_str!("../client/rooms.css");

/// What the build step produced.
pub struct Built {
    pub js: PathBuf,
    pub css: PathBuf,
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OUT_DIR").expect("build scripts are given OUT_DIR"))
}

fn tell_cargo(path: &Path) {
    println!("cargo::rerun-if-changed={}", path.display());
}

/// Compiles `<client>/main.ts` and appends any `<client>/*.css` to the
/// shared stylesheet.
///
/// The tool's TypeScript imports the shared half as `./rooms`; both are
/// copied into one directory first so that the import resolves without the
/// tool having to know where this crate lives.
///
/// `esbuild` does the compiling. `tsc` checks the types if it is on the
/// path, and the build says so loudly when it is not, rather than quietly
/// shipping TypeScript nobody has checked.
pub fn bundle(client: &str) -> Built {
    let (js, css) = gather(client, true);
    Built {
        js,
        css: css.expect("the shared stylesheet was asked for"),
    }
}

/// The same, for a tool with a look of its own. Petrov Day is a dark
/// console with a siren on it and shares none of the stylesheet, but it
/// shares every line of the TypeScript underneath.
pub fn script(client: &str) -> PathBuf {
    gather(client, false).0
}

fn gather(client: &str, styled: bool) -> (PathBuf, Option<PathBuf>) {
    let out = out_dir();
    let work = out.join("client");
    fs::create_dir_all(&work).expect("can write to OUT_DIR");

    fs::write(work.join("rooms.ts"), ROOMS_TS).expect("writes the shared TypeScript");

    let client = PathBuf::from(client);
    tell_cargo(&client);
    let mut css = if styled {
        ROOMS_CSS.to_string()
    } else {
        String::new()
    };
    for entry in fs::read_dir(&client).expect("the client directory is there") {
        let path = entry.expect("readable").path();
        tell_cargo(&path);
        match path.extension().and_then(|e| e.to_str()) {
            Some("ts") => {
                let to = work.join(path.file_name().expect("a file name"));
                fs::copy(&path, &to).expect("copies the tool's TypeScript");
            }
            Some("css") => {
                css.push('\n');
                css.push_str(&fs::read_to_string(&path).expect("readable stylesheet"));
            }
            _ => {}
        }
    }

    let written = styled.then(|| {
        let path = out.join("app.css");
        fs::write(&path, css).expect("writes the stylesheet");
        path
    });

    typecheck(&work);
    let js = out.join("app.js");
    compile(&work.join("main.ts"), &js);

    (js, written)
}

/// Where a program is, looking the way each platform spells it. npm
/// installs esbuild and tsc as `.cmd` shims on Windows, and Windows will
/// not start one of those as a program.
fn which(program: &str) -> Option<PathBuf> {
    let endings: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        endings
            .iter()
            .map(|ending| dir.join(format!("{program}{ending}")))
            .find(|candidate| candidate.is_file())
    })
}

/// Runs a program, or says it is not there.
fn run(program: &str, args: &[&str]) -> Option<std::process::Output> {
    let found = which(program)?;
    let shim = found
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let output = if shim {
        // A shim is a batch script, so the shell has to run it.
        Command::new("cmd")
            .arg("/C")
            .arg(&found)
            .args(args)
            .output()
    } else {
        Command::new(&found).args(args).output()
    };
    output.ok()
}

fn typecheck(work: &Path) {
    let config = work.join("tsconfig.json");
    fs::write(
        &config,
        r#"{
  "compilerOptions": {
    "target": "es2020",
    "module": "esnext",
    "moduleResolution": "bundler",
    "lib": ["es2020", "dom", "dom.iterable"],
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "noImplicitReturns": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["*.ts"]
}
"#,
    )
    .expect("writes the TypeScript settings");

    let Some(output) = run("tsc", &["--noEmit", "-p", &config.display().to_string()]) else {
        println!(
            "cargo::warning=tsc is not on the path, so the TypeScript was \
             compiled without being checked. `nix develop` has it."
        );
        return;
    };
    if !output.status.success() {
        panic!(
            "the TypeScript does not typecheck:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn compile(entry: &Path, to: &Path) {
    let output = run(
        "esbuild",
        &[
            &entry.display().to_string(),
            "--bundle",
            "--minify",
            "--format=iife",
            // What a phone that is a few years old runs.
            "--target=es2020",
            &format!("--outfile={}", to.display()),
        ],
    )
    .expect("esbuild is on the path; `nix develop` has it");
    if !output.status.success() {
        panic!(
            "esbuild could not compile the page:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_half_is_typescript_and_is_here() {
        assert!(ROOMS_TS.contains("export function watch"));
        assert!(ROOMS_CSS.contains("--paper"));
    }

    /// The point of the whole arrangement: no JavaScript is written by
    /// hand, so none is checked in.
    #[test]
    fn nothing_here_is_a_javascript_file() {
        let client = Path::new(env!("CARGO_MANIFEST_DIR")).join("client");
        for entry in fs::read_dir(client).expect("the client directory is there") {
            let path = entry.expect("readable").path();
            let kind = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            assert!(
                kind == "ts" || kind == "css",
                "{} is neither TypeScript nor a stylesheet",
                path.display()
            );
        }
    }
}
