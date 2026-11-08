use crate::util::text::fuzzy_match;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    OpenUrl,
    Search,
    Home,
    Subscriptions,
    Library,
    Refresh,
    ImportCookies,
    ImportSubscriptions,
    ToggleTheme,
    ToggleImages,
    ToggleVideo,
    StopPlayback,
    ClearCache,
    Help,
    Quit,
}

pub struct CommandDef {
    pub name: &'static str,
    pub action: Action,
}

pub const COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "Open URL or search",
        action: Action::OpenUrl,
    },
    CommandDef {
        name: "Search YouTube",
        action: Action::Search,
    },
    CommandDef {
        name: "Go home",
        action: Action::Home,
    },
    CommandDef {
        name: "Subscriptions",
        action: Action::Subscriptions,
    },
    CommandDef {
        name: "Library",
        action: Action::Library,
    },
    CommandDef {
        name: "Refresh",
        action: Action::Refresh,
    },
    CommandDef {
        name: "Import cookies.txt",
        action: Action::ImportCookies,
    },
    CommandDef {
        name: "Import subscriptions",
        action: Action::ImportSubscriptions,
    },
    CommandDef {
        name: "Toggle theme",
        action: Action::ToggleTheme,
    },
    CommandDef {
        name: "Toggle images",
        action: Action::ToggleImages,
    },
    CommandDef {
        name: "Toggle video window",
        action: Action::ToggleVideo,
    },
    CommandDef {
        name: "Stop playback",
        action: Action::StopPlayback,
    },
    CommandDef {
        name: "Clear cache",
        action: Action::ClearCache,
    },
    CommandDef {
        name: "Keyboard shortcuts",
        action: Action::Help,
    },
    CommandDef {
        name: "Quit",
        action: Action::Quit,
    },
];

pub fn filtered(input: &str) -> Vec<&'static CommandDef> {
    let mut matches: Vec<(i32, &'static CommandDef)> = COMMANDS
        .iter()
        .filter_map(|command| fuzzy_match(input, command.name).map(|(score, _)| (score, command)))
        .collect();
    matches.sort_by_key(|item| std::cmp::Reverse(item.0));
    matches.into_iter().map(|(_, command)| command).collect()
}
