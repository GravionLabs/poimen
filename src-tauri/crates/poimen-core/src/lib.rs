//! Project-side logic shared by the app, the console and `poimen-server`, free of Tauri: the
//! project file, trust in it, space templates, remote parsing and the pure part of the pane
//! layout, plus what they need (task modes, harnesses, agent actions, the GUI `PATH`, and the
//! settings parsers and the in-place config editor), and the agent-events reducer of the
//! Deliver panel.

pub mod agent_actions;
pub mod agent_events;
pub mod copilot;
pub mod git;
pub mod github;
pub mod gui_path;
pub mod harness;
pub mod layout;
pub mod project;
pub mod remote;
pub mod repos;
pub mod schemes;
pub mod settings;
pub mod snapshot;
pub mod space_layout;
pub mod task_start;
pub mod tasks;
pub mod templates;
pub mod transcripts;
pub mod trust;
pub mod worktree;
