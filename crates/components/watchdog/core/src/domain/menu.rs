//! Detect Claude Code's interactive rate-limit menu in captured pane text and
//! report which option the cursor is currently on.
//!
//! Claude Code presents a 3-option dialog when it hits the rate limit:
//! ```text
//! What do you want to do?
//!
//! ❯ 1. Stop and wait for limit to reset
//!   2. Upgrade your plan
//!   3. Upgrade to Team plan
//! ```
//! The watchdog must always pick option 1 — `Stop and wait for limit to reset`
//! — confirm with Enter, and only then send the resume text. To know how many
//! `Up` presses are needed, we need the current cursor position.

const OPTION_TEXTS: [(u8, &str); 3] = [
    (1, "Stop and wait for limit to reset"),
    (2, "Upgrade your plan"),
    (3, "Upgrade to Team plan"),
];

/// State of the rate-limit menu detected in a captured pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitMenu {
    /// 1-based position of the cursor (1, 2, or 3).
    pub cursor: u8,
}

impl RateLimitMenu {
    /// How many `Up` key presses are needed to land the cursor on option 1.
    #[must_use]
    pub fn ups_to_top(self) -> u8 {
        self.cursor.saturating_sub(1)
    }
}

/// Try to detect the menu and the cursor position in the pane text. Returns
/// `None` when fewer than all three option lines are visible — that's our
/// signal the menu is not currently open.
#[must_use]
pub fn detect_rate_limit_menu(pane_text: &str) -> Option<RateLimitMenu> {
    let mut found: [Option<bool>; 3] = [None; 3];

    for line in pane_text.lines() {
        if let Some((n, has_marker)) = parse_option_line(line) {
            let idx = (n - 1) as usize;
            found[idx] = Some(has_marker);
        }
    }

    if found.iter().any(Option::is_none) {
        return None;
    }

    let cursor = found
        .iter()
        .enumerate()
        .find_map(|(idx, marker)| {
            if matches!(marker, Some(true)) {
                u8::try_from(idx + 1).ok()
            } else {
                None
            }
        })
        // Menu visible but no marker found — assume option 1 is the default.
        .unwrap_or(1);

    Some(RateLimitMenu { cursor })
}

/// If `line` looks like one of the three menu options, return `(option_number,
/// has_cursor_marker)`. The cursor marker is any non-whitespace glyph appearing
/// before the digit (e.g. `❯`, `>`, `)`, `▸`).
fn parse_option_line(line: &str) -> Option<(u8, bool)> {
    for (n, expected_text) in OPTION_TEXTS {
        let needle = format!("{n}. ");
        let Some(pos) = line.find(&needle) else {
            continue;
        };
        let prefix = &line[..pos];
        // Reject lines where the digit is buried deep — keeps prose with
        // "step 1. do X" from masquerading as a menu option.
        if prefix.chars().count() > 4 {
            continue;
        }
        let suffix = &line[pos + needle.len()..];
        if !suffix.starts_with(expected_text) {
            continue;
        }
        let has_marker = prefix.chars().any(|c| !c.is_whitespace());
        return Some((n, has_marker));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE_CURSOR_ON_1: &str = "\
What do you want to do?

❯ 1. Stop and wait for limit to reset
  2. Upgrade your plan
  3. Upgrade to Team plan

Enter to confirm · Esc to cancel
";

    const PANE_CURSOR_ON_2: &str = "\
What do you want to do?

  1. Stop and wait for limit to reset
❯ 2. Upgrade your plan
  3. Upgrade to Team plan

Enter to confirm · Esc to cancel
";

    const PANE_CURSOR_ON_3: &str = "\
What do you want to do?

  1. Stop and wait for limit to reset
  2. Upgrade your plan
> 3. Upgrade to Team plan

Enter to confirm · Esc to cancel
";

    #[test]
    fn detects_cursor_on_option_1() {
        let menu = detect_rate_limit_menu(PANE_CURSOR_ON_1).expect("menu visible");
        assert_eq!(menu.cursor, 1);
        assert_eq!(menu.ups_to_top(), 0);
    }

    #[test]
    fn detects_cursor_on_option_2() {
        let menu = detect_rate_limit_menu(PANE_CURSOR_ON_2).expect("menu visible");
        assert_eq!(menu.cursor, 2);
        assert_eq!(menu.ups_to_top(), 1);
    }

    #[test]
    fn detects_cursor_on_option_3() {
        let menu = detect_rate_limit_menu(PANE_CURSOR_ON_3).expect("menu visible");
        assert_eq!(menu.cursor, 3);
        assert_eq!(menu.ups_to_top(), 2);
    }

    #[test]
    fn returns_none_when_menu_not_present() {
        assert!(detect_rate_limit_menu("just chatter, no menu here\n").is_none());
    }

    #[test]
    fn returns_none_when_only_some_options_visible() {
        let pane = "  1. Stop and wait for limit to reset\n  2. Upgrade your plan\n";
        assert!(detect_rate_limit_menu(pane).is_none());
    }

    #[test]
    fn ignores_prose_with_step_numbers() {
        let pane = "Did you remember step 1. Stop and wait for limit to reset before step 2. Upgrade your plan and 3. Upgrade to Team plan?\n";
        assert!(detect_rate_limit_menu(pane).is_none());
    }

    #[test]
    fn defaults_to_option_1_when_no_marker_visible() {
        let pane = "\
  1. Stop and wait for limit to reset
  2. Upgrade your plan
  3. Upgrade to Team plan
";
        let menu = detect_rate_limit_menu(pane).expect("menu visible");
        assert_eq!(menu.cursor, 1);
    }
}
