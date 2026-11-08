//! The workspaces, declared as data (design.md, Decided; corpus/interface/DESIGN.md): the
//! ribbon of each as tabs › sections › controls (a registry command, or an action of the
//! interface itself), and the Simulate workspace as a stepper with a primary command.
//! SolidCraft's planned plugin workspaces are host-drawn from such declarations, so these
//! can move over later; the drawing code does not.

use crate::theme::{self, Accent};

/// What a ribbon control does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Runs a command of the registry (`optobench_commands`) by id.
    Command(&'static str),
    /// Does something in the interface itself.
    Action(Action),
}

/// The interface's own actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open a design (a layout or a sidecar).
    Open,
    /// Read the open design again from disk.
    Reload,
    /// Fit the view to the scene.
    Fit,
    /// Look down the scene's z axis.
    Top,
    /// Look along +y.
    Front,
    /// Look along −x.
    Right,
    /// The isometric view.
    Isometric,
    /// Show or hide the traced rays.
    Rays,
}

impl Action {
    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            Action::Open => "Open",
            Action::Reload => "Reload",
            Action::Fit => "Fit",
            Action::Top => "Top",
            Action::Front => "Front",
            Action::Right => "Right",
            Action::Isometric => "Isometric",
            Action::Rays => "Rays",
        }
    }
}

/// A control in a section, promoted to a button or only in the section's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// What it does.
    pub control: Control,
    /// Shown as a button (else in the section's menu only).
    pub promoted: bool,
}

const fn button(control: Control) -> Item {
    Item {
        control,
        promoted: true,
    }
}

const fn menu(control: Control) -> Item {
    Item {
        control,
        promoted: false,
    }
}

const fn command(id: &'static str) -> Control {
    Control::Command(id)
}

/// A labelled group of controls in a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// Its label, under the buttons.
    pub name: &'static str,
    /// Its controls, in order.
    pub items: &'static [Item],
}

/// A ribbon tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    /// Its label.
    pub name: &'static str,
    /// Its sections.
    pub sections: &'static [Section],
}

/// A step of a stepper workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stage {
    /// Its label.
    pub name: &'static str,
    /// What it is for, shown in the inspector.
    pub help: &'static str,
}

/// How a workspace's ribbon is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Tabs of sections, as SolidCraft's Design workspace.
    Ribbon(&'static [Tab]),
    /// Numbered stages and a primary command, as SolidCraft's Render workspace.
    Stages {
        /// The stages, in order.
        stages: &'static [Stage],
        /// The primary command, at the right.
        primary: Control,
        /// Tools beside the stages.
        tools: &'static [Item],
    },
}

/// A workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workspace {
    /// Its id.
    pub id: &'static str,
    /// Its name on the tile.
    pub name: &'static str,
    /// Its accent.
    pub accent: Accent,
    /// Its ribbon.
    pub layout: Layout,
}

const VIEW_SECTION: Section = Section {
    name: "VIEW",
    items: &[
        button(Control::Action(Action::Fit)),
        button(Control::Action(Action::Isometric)),
        menu(Control::Action(Action::Top)),
        menu(Control::Action(Action::Front)),
        menu(Control::Action(Action::Right)),
        button(Control::Action(Action::Rays)),
    ],
};

const FILE_SECTION: Section = Section {
    name: "FILE",
    items: &[
        button(Control::Action(Action::Open)),
        button(Control::Action(Action::Reload)),
    ],
};

const LIBRARY_SECTION: Section = Section {
    name: "LIBRARY",
    items: &[
        button(command("parts")),
        button(command("curves")),
        button(command("materials")),
        menu(command("import-part")),
        menu(command("import-curve")),
        menu(command("import-material")),
    ],
};

/// Every workspace, in the order the switcher lists them.
pub const WORKSPACES: [Workspace; 3] = [
    Workspace {
        id: "cad",
        name: "CAD",
        accent: theme::CAD,
        layout: Layout::Ribbon(&[
            Tab {
                name: "Scene",
                sections: &[
                    FILE_SECTION,
                    Section {
                        name: "INSPECT",
                        items: &[button(command("bodies")), button(command("trace"))],
                    },
                    VIEW_SECTION,
                ],
            },
            Tab {
                name: "Align",
                sections: &[
                    Section {
                        name: "ALIGN",
                        items: &[button(command("optimize")), button(command("check-moves"))],
                    },
                    Section {
                        name: "SHARE",
                        items: &[button(command("embed"))],
                    },
                ],
            },
            Tab {
                name: "Library",
                sections: &[LIBRARY_SECTION],
            },
        ]),
    },
    Workspace {
        id: "layout",
        name: "Layout",
        accent: theme::LAYOUT,
        layout: Layout::Ribbon(&[
            Tab {
                name: "Layout",
                sections: &[
                    FILE_SECTION,
                    Section {
                        name: "TRACE",
                        items: &[button(command("trace")), button(command("optimize"))],
                    },
                    Section {
                        name: "OUTPUT",
                        items: &[
                            button(command("export-placements")),
                            button(command("embed")),
                        ],
                    },
                    VIEW_SECTION,
                ],
            },
            Tab {
                name: "Library",
                sections: &[LIBRARY_SECTION],
            },
        ]),
    },
    Workspace {
        id: "simulate",
        name: "Simulate",
        accent: theme::SIMULATE,
        layout: Layout::Stages {
            stages: &[
                Stage {
                    name: "Sources",
                    help: "The light: kind, wavelength or spectrum, power and where it starts.",
                },
                Stage {
                    name: "Optics",
                    help: "Lenses, mirrors and filters: glasses, coatings and library parts.",
                },
                Stage {
                    name: "Detectors & merit",
                    help: "Where the light is measured, and what alignment should achieve.",
                },
                Stage {
                    name: "Analyse",
                    help: "Trace, then read the power ledger, the detectors and the clipping.",
                },
            ],
            primary: command("trace"),
            tools: &[
                button(command("optimize")),
                button(Control::Action(Action::Rays)),
                button(Control::Action(Action::Fit)),
            ],
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command a workspace refers to.
    fn commands() -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut add = |item: &Item| {
            if let Control::Command(id) = item.control {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        };
        for w in &WORKSPACES {
            match w.layout {
                Layout::Ribbon(tabs) => tabs
                    .iter()
                    .flat_map(|t| t.sections)
                    .flat_map(|s| s.items)
                    .for_each(&mut add),
                Layout::Stages { primary, tools, .. } => {
                    add(&button(primary));
                    tools.iter().for_each(&mut add);
                }
            }
        }
        out
    }

    #[test]
    fn every_command_in_a_workspace_is_in_the_registry() {
        let registry = optobench_commands::Registry::standard();
        for id in commands() {
            let command = registry
                .get(id)
                .unwrap_or_else(|| panic!("`{id}` is not a command"));
            assert!(
                !command.label.is_empty() && !command.icon.is_empty(),
                "{id}"
            );
        }
        assert!(commands().contains(&"trace"));
    }

    #[test]
    fn workspaces_are_distinct_and_complete() {
        let ids: Vec<&str> = WORKSPACES.iter().map(|w| w.id).collect();
        assert_eq!(ids, ["cad", "layout", "simulate"]);
        let accents: Vec<_> = WORKSPACES.iter().map(|w| w.accent.fill).collect();
        assert!(accents[0] != accents[1] && accents[1] != accents[2] && accents[0] != accents[2]);
        for w in &WORKSPACES {
            if let Layout::Ribbon(tabs) = w.layout {
                assert!(!tabs.is_empty());
                for tab in tabs {
                    assert!(
                        tab.sections
                            .iter()
                            .all(|s| s.items.iter().any(|i| i.promoted)),
                        "{}",
                        tab.name
                    );
                }
            }
        }
    }
}
