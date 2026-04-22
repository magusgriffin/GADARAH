//! Tab definitions for the installation wizard.
//!
//! The wizard is strictly linear — the user moves Welcome → License →
//! Components → Install → Finish. The breadcrumb lets them jump back to an
//! earlier tab only up to their furthest-reached step.

pub mod components;
pub mod finish;
pub mod install;
pub mod license;
pub mod welcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardTab {
    Welcome,
    License,
    Components,
    Install,
    Finish,
}

impl WizardTab {
    pub const ORDER: [Self; 5] = [
        Self::Welcome,
        Self::License,
        Self::Components,
        Self::Install,
        Self::Finish,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::License => "License",
            Self::Components => "Components",
            Self::Install => "Install",
            Self::Finish => "Finish",
        }
    }

    pub fn step_number(self) -> u8 {
        Self::ORDER.iter().position(|t| *t == self).unwrap_or(0) as u8 + 1
    }

    pub fn next(self) -> Option<Self> {
        let idx = Self::ORDER.iter().position(|t| *t == self)?;
        Self::ORDER.get(idx + 1).copied()
    }

    pub fn prev(self) -> Option<Self> {
        let idx = Self::ORDER.iter().position(|t| *t == self)?;
        if idx == 0 {
            None
        } else {
            Self::ORDER.get(idx - 1).copied()
        }
    }
}
