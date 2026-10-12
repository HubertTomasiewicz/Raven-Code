//! Project kinds (ecosystems) understood by Raven Pico.

use std::fmt;
use std::str::FromStr;

/// How certain the detector is about a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Forced by configuration or an explicit CLI flag.
    Explicit,
    /// A canonical, unambiguous marker was found (e.g. `Cargo.toml`).
    High,
    /// A weaker marker was found (e.g. `requirements.txt`).
    Medium,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Explicit => "explicit",
            Confidence::High => "high",
            Confidence::Medium => "medium",
        }
    }
}

/// The ecosystems Raven Pico can detect and operate on.
///
/// A *project kind* identifies the ecosystem, not necessarily a single build
/// system. For example [`ProjectKind::VisualStudio`] covers `.sln`, `.slnx`
/// and `.vcxproj` projects driven by MSBuild, while
/// [`ProjectKind::DotNet`] covers `.csproj`/`.fsproj` projects driven by
/// `dotnet`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectKind {
    Rust,
    CMake,
    VisualStudio,
    DotNet,
    Python,
    Godot,
    Maven,
    Gradle,
    Lua,
    /// A project driven purely by `raven-pico.toml` commands.
    Custom,
}

impl ProjectKind {
    /// Canonical kebab-case identifier used on the CLI and in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectKind::Rust => "rust",
            ProjectKind::CMake => "cmake",
            ProjectKind::VisualStudio => "visual-studio",
            ProjectKind::DotNet => "dotnet",
            ProjectKind::Python => "python",
            ProjectKind::Godot => "godot",
            ProjectKind::Maven => "maven",
            ProjectKind::Gradle => "gradle",
            ProjectKind::Lua => "lua",
            ProjectKind::Custom => "custom",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            ProjectKind::Rust => "Rust (Cargo)",
            ProjectKind::CMake => "C/C++ (CMake)",
            ProjectKind::VisualStudio => "Visual Studio (MSBuild)",
            ProjectKind::DotNet => ".NET (dotnet)",
            ProjectKind::Python => "Python",
            ProjectKind::Godot => "Godot",
            ProjectKind::Maven => "Java (Maven)",
            ProjectKind::Gradle => "Java (Gradle)",
            ProjectKind::Lua => "Lua",
            ProjectKind::Custom => "Custom (raven-pico.toml)",
        }
    }

    /// All kinds, in canonical order.
    pub fn all() -> &'static [ProjectKind] {
        &[
            ProjectKind::Rust,
            ProjectKind::CMake,
            ProjectKind::VisualStudio,
            ProjectKind::DotNet,
            ProjectKind::Python,
            ProjectKind::Godot,
            ProjectKind::Maven,
            ProjectKind::Gradle,
            ProjectKind::Lua,
            ProjectKind::Custom,
        ]
    }

    /// Parses a kind from a user string, accepting common aliases.
    pub fn parse(s: &str) -> Option<ProjectKind> {
        let lower = s.trim().to_ascii_lowercase();
        let trimmed = lower.trim_start_matches('.');
        match trimmed {
            "rust" | "cargo" => Some(ProjectKind::Rust),
            "cmake" | "c" | "cpp" | "c++" => Some(ProjectKind::CMake),
            "visual-studio" | "vs" | "msbuild" | "vcxproj" | "sln" => {
                Some(ProjectKind::VisualStudio)
            }
            "dotnet" | "net" | "csharp" | "csproj" | "fsharp" => Some(ProjectKind::DotNet),
            "python" | "py" => Some(ProjectKind::Python),
            "godot" => Some(ProjectKind::Godot),
            "maven" => Some(ProjectKind::Maven),
            "gradle" => Some(ProjectKind::Gradle),
            "lua" => Some(ProjectKind::Lua),
            "custom" => Some(ProjectKind::Custom),
            _ => None,
        }
    }

    /// Comma-separated list of accepted aliases, used in error messages.
    pub fn aliases() -> &'static str {
        "rust, cmake, visual-studio, dotnet, python, godot, maven, gradle, lua, custom"
    }
}

impl fmt::Display for ProjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for ProjectKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProjectKind::parse(s).ok_or_else(|| {
            format!(
                "unknown project kind '{s}'; expected one of: {}",
                ProjectKind::aliases()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases() {
        assert_eq!(ProjectKind::parse("cargo"), Some(ProjectKind::Rust));
        assert_eq!(ProjectKind::parse("C++"), Some(ProjectKind::CMake));
        assert_eq!(ProjectKind::parse(".csproj"), Some(ProjectKind::DotNet));
        assert_eq!(
            ProjectKind::parse("MSBuild"),
            Some(ProjectKind::VisualStudio)
        );
        assert_eq!(ProjectKind::parse("nope"), None);
    }

    #[test]
    fn json_roundtrip_is_kebab_case() {
        let json = serde_json::to_string(&ProjectKind::VisualStudio).unwrap();
        assert_eq!(json, "\"visual-studio\"");
        let back: ProjectKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ProjectKind::VisualStudio);
    }
}
