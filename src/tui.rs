//! Custom TUI widgets using crossterm.
//!
//! Provides [`text_input`], [`search_select`], and [`feature_select`] as
//! replacements for the `inquire` crate, giving full control over key
//! handling, styling, and layout.

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    style::{self, Stylize},
    terminal, ExecutableCommand, QueueableCommand,
};
use std::io::{self, Write};

// ── Helpers ──────────────────────────────────────────────────────────────

/// Enter raw mode and hide the cursor; returns a guard that restores state
/// when dropped.
struct RawGuard {
    cursor_hidden: bool,
}

impl RawGuard {
    fn enter(hide_cursor: bool) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if hide_cursor {
            io::stdout().execute(cursor::Hide)?;
        }
        Ok(RawGuard {
            cursor_hidden: hide_cursor,
        })
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        if self.cursor_hidden {
            let _ = io::stdout().execute(cursor::Show);
        }
        let _ = terminal::disable_raw_mode();
    }
}

/// Move to the beginning of the current line, then clear everything below
/// (inclusive). Use this before a full redraw.
fn move_to_start_and_clear(out: &mut impl Write) -> io::Result<()> {
    out.queue(cursor::MoveToColumn(0))?;
    out.queue(terminal::Clear(terminal::ClearType::FromCursorDown))?;
    Ok(())
}

/// Move up `n` lines from the current position.
fn move_up(out: &mut impl Write, n: u16) -> io::Result<()> {
    if n > 0 {
        out.queue(cursor::MoveUp(n))?;
    }
    Ok(())
}

/// Print the final "✔ prompt value" line after a widget confirms.
fn print_confirm(out: &mut impl Write, prompt: &str, value: &str) -> io::Result<()> {
    out.queue(style::Print(style::style("✔ ").green().bold()))?;
    out.queue(style::Print(style::style(prompt).bold()))?;
    out.queue(style::Print(" "))?;
    out.queue(style::Print(style::style(value).cyan()))?;
    out.queue(style::Print("\r\n"))?;
    out.flush()?;
    Ok(())
}

// ── confirm ──────────────────────────────────────────────────────────────

/// Prompt the user with a yes/no question. Returns `true` for yes.
///
/// Display: `? prompt [Y/n] _`
/// Enter or 'y'/'Y' → true, 'n'/'N' → false.
pub fn confirm(prompt: &str) -> Result<bool, String> {
    confirm_inner(prompt).map_err(|e| e.to_string())
}

fn confirm_inner(prompt: &str) -> io::Result<bool> {
    let _guard = RawGuard::enter(false)?;
    let mut out = io::stdout();

    out.queue(style::SetForegroundColor(style::Color::Green))?;
    out.queue(style::Print("? "))?;
    out.queue(style::ResetColor)?;
    out.queue(style::Print(prompt))?;
    out.queue(style::Print(" "))?;
    out.queue(style::SetForegroundColor(style::Color::DarkGrey))?;
    out.queue(style::Print("[Y/n] "))?;
    out.queue(style::ResetColor)?;
    out.flush()?;

    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    out.queue(style::Print("Yes\r\n"))?;
                    out.flush()?;
                    return Ok(true);
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    out.queue(style::Print("No\r\n"))?;
                    out.flush()?;
                    return Ok(false);
                }
                KeyCode::Esc => {
                    out.queue(style::Print("No\r\n"))?;
                    out.flush()?;
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

// ── text_input ───────────────────────────────────────────────────────────

/// Prompt for a single line of text with an optional default.
///
/// Returns the entered string (or default if the user just pressed Enter).
pub fn text_input(prompt: &str, default: &str) -> Result<String, String> {
    text_input_inner(prompt, default).map_err(|e| e.to_string())
}

fn text_input_inner(prompt: &str, default: &str) -> io::Result<String> {
    // Keep cursor VISIBLE for text input so user sees where they type.
    let _guard = RawGuard::enter(false)?;
    let mut out = io::stdout();
    let mut buf = String::new();

    render_text_input(&mut out, prompt, &buf, default)?;

    loop {
        if let Event::Key(key) = event::read()? {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                drop(_guard);
                std::process::exit(130);
            }
            match key.code {
                KeyCode::Enter => {
                    let result = if buf.is_empty() {
                        default.to_string()
                    } else {
                        buf
                    };
                    // Overwrite prompt line with confirmed version
                    out.queue(cursor::MoveToColumn(0))?;
                    out.queue(terminal::Clear(terminal::ClearType::CurrentLine))?;
                    print_confirm(&mut out, prompt, &result)?;
                    return Ok(result);
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                }
                _ => {}
            }
            render_text_input(&mut out, prompt, &buf, default)?;
        }
    }
}

fn render_text_input(
    out: &mut io::Stdout,
    prompt: &str,
    buf: &str,
    default: &str,
) -> io::Result<()> {
    out.queue(cursor::MoveToColumn(0))?;
    out.queue(terminal::Clear(terminal::ClearType::CurrentLine))?;
    out.queue(style::Print(style::style("? ").green().bold()))?;
    out.queue(style::Print(style::style(prompt).bold()))?;
    out.queue(style::Print(" "))?;
    if buf.is_empty() {
        out.queue(style::Print(style::style(default).dark_grey()))?;
        // Position cursor at start of input area (before placeholder)
        let col = 2 + prompt.len() + 1; // "? " + prompt + " "
        out.queue(cursor::MoveToColumn(col as u16))?;
    } else {
        out.queue(style::Print(buf))?;
    }
    out.flush()?;
    Ok(())
}

// ── search_select (single) ──────────────────────────────────────────────

const VISIBLE_RESULTS: usize = 6;

/// Single-select with type-to-search. Returns the selected item.
///
/// The top filtered result is always highlighted (blue). Press Enter/Space
/// to pick it. No arrow-key navigation — you refine by typing.
pub fn search_select(prompt: &str, items: &[String], help: &str) -> Result<String, String> {
    search_select_inner(prompt, items, help).map_err(|e| e.to_string())
}

fn search_select_inner(prompt: &str, items: &[String], help: &str) -> io::Result<String> {
    // Keep cursor visible so user sees where they type in the search box.
    let _guard = RawGuard::enter(false)?;
    let mut out = io::stdout();
    let mut query = String::new();
    let mut last_lines: u16 = 0;

    last_lines = render_search(&mut out, prompt, &query, items, &[], help, last_lines)?;

    loop {
        if let Event::Key(key) = event::read()? {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                drop(_guard);
                std::process::exit(130);
            }
            match key.code {
                KeyCode::Enter => {
                    let filtered = filter(items, &query);
                    if let Some(selected) = filtered.first() {
                        // Clear widget and print confirmed line
                        move_up(&mut out, last_lines)?;
                        move_to_start_and_clear(&mut out)?;
                        print_confirm(&mut out, prompt, selected)?;
                        return Ok(selected.to_string());
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(c) => {
                    query.push(c);
                }
                _ => {}
            }
            last_lines =
                render_search(&mut out, prompt, &query, items, &[], help, last_lines)?;
        }
    }
}

fn filter<'a>(items: &'a [String], query: &str) -> Vec<&'a String> {
    if query.is_empty() {
        return items.iter().collect();
    }
    let q = query.to_lowercase();
    // Split into starts-with and contains-but-not-starts-with,
    // preserving the original popularity order within each group.
    let mut starts: Vec<&String> = Vec::new();
    let mut contains: Vec<&String> = Vec::new();
    for item in items {
        let lower = item.to_lowercase();
        if lower.starts_with(&q) {
            starts.push(item);
        } else if lower.contains(&q) {
            contains.push(item);
        }
    }
    starts.extend(contains);
    starts
}

/// Render the search widget. Returns total line count (below the starting
/// row) so the next redraw knows how far to move up.
fn render_search(
    out: &mut io::Stdout,
    prompt: &str,
    query: &str,
    items: &[String],
    selected: &[String],
    help: &str,
    prev_lines: u16,
) -> io::Result<u16> {
    // Go back to the top of the widget
    move_up(out, prev_lines)?;
    move_to_start_and_clear(out)?;

    // Prompt line
    out.queue(style::Print(style::style("? ").green().bold()))?;
    out.queue(style::Print(style::style(prompt).bold()))?;
    out.queue(style::Print(" "))?;
    if query.is_empty() {
        out.queue(style::Print(style::style("(type to search)").dark_grey()))?;
    } else {
        out.queue(style::Print(query))?;
    }
    out.queue(style::Print("\r\n"))?;

    let filtered = filter(items, query);
    let shown: Vec<&String> = filtered.iter().take(VISIBLE_RESULTS).copied().collect();

    let mut lines: u16 = 1; // prompt line
    for (i, item) in shown.iter().enumerate() {
        let is_selected = selected.iter().any(|s| s == *item);
        if i == 0 {
            out.queue(style::Print(style::style(format!("  {item}")).blue()))?;
        } else if is_selected {
            out.queue(style::Print(style::style(format!("  {item}")).green()))?;
        } else {
            out.queue(style::Print(format!("  {item}")))?;
        }
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    if !help.is_empty() {
        out.queue(style::Print(style::style(format!("  {help}")).dark_grey()))?;
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    // Move cursor back to the prompt line so it sits after the query text.
    // After printing `lines` lines of \r\n, cursor is `lines` rows below start.
    move_up(out, lines)?;
    let col = 2 + prompt.len() + 1 + query.len(); // "? " + prompt + " " + query
    out.queue(cursor::MoveToColumn(col as u16))?;
    out.flush()?;
    // Cursor is parked at prompt line (row 0), so next re-render needs 0 move-up.
    // Clear(FromCursorDown) will wipe all the content below.
    Ok(0)
}

// ── feature_select ──────────────────────────────────────────────────────

/// Item in the feature selector: either a toggleable feature or the action
/// button ("Create!" / "Done!").
enum FeatureRow<'a> {
    Feature {
        key: &'a str,
        label: &'a str,
        desc: &'a str,
    },
    Action(&'a str),
}

/// Multi-select for features with a final action button.
///
/// Arrow keys move the cursor. Space *and* Enter toggle items. On
/// the action button, both Space and Enter confirm.
///
/// Styling: cursor → blue text, checked (no cursor) → green, unchecked → white,
/// action button → blue when cursor is on it, white otherwise.
///
/// `pre_checked` — keys that start already selected.
/// `locked` — keys that are already enabled and cannot be toggled (shows
/// sorry message; for `plyx add`).
/// `action_label` — e.g. `"Create!"` or `"Done!"`.
pub fn feature_select(
    prompt: &str,
    features: &[(&str, &str, &str)],
    help: &str,
    pre_checked: &[&str],
    locked: &[&str],
    action_label: &str,
) -> Result<Vec<String>, String> {
    feature_select_inner(prompt, features, help, pre_checked, locked, action_label)
        .map_err(|e| e.to_string())
}

fn feature_select_inner(
    prompt: &str,
    features: &[(&str, &str, &str)],
    help: &str,
    pre_checked: &[&str],
    locked: &[&str],
    action_label: &str,
) -> io::Result<Vec<String>> {
    let _guard = RawGuard::enter(true)?; // hide cursor for arrow-key nav
    let mut out = io::stdout();

    let mut rows: Vec<FeatureRow> = features
        .iter()
        .map(|&(key, label, desc)| FeatureRow::Feature { key, label, desc })
        .collect();
    rows.push(FeatureRow::Action(action_label));

    let mut cursor: usize = 0;
    let mut checked: Vec<bool> = features
        .iter()
        .map(|(key, _, _)| pre_checked.contains(key))
        .collect();

    let mut sorry_index: Option<usize> = None;
    let mut last_lines: u16 = 0;

    last_lines = render_features(
        &mut out, prompt, &rows, cursor, &checked, locked, sorry_index, help, last_lines,
    )?;

    loop {
        if let Event::Key(key) = event::read()? {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                drop(_guard);
                std::process::exit(130);
            }

            match key.code {
                KeyCode::Up => {
                    if cursor > 0 {
                        cursor -= 1;
                    }
                    sorry_index = None;
                }
                KeyCode::Down => {
                    if cursor + 1 < rows.len() {
                        cursor += 1;
                    }
                    sorry_index = None;
                }
                KeyCode::Char(' ') | KeyCode::Enter => match &rows[cursor] {
                    FeatureRow::Feature { key: fkey, .. } => {
                        if locked.contains(fkey) {
                            sorry_index = Some(cursor);
                        } else {
                            if let Some(c) = checked.get_mut(cursor) {
                                *c = !*c;
                            }
                            sorry_index = None;
                        }
                    }
                    FeatureRow::Action(_) => {
                        let result: Vec<String> = features
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| checked.get(*i).copied().unwrap_or(false))
                            .map(|(_, (key, _, _))| key.to_string())
                            .collect();

                        move_up(&mut out, last_lines)?;
                        move_to_start_and_clear(&mut out)?;
                        let display = if result.is_empty() {
                            "(none)".to_string()
                        } else {
                            features
                                .iter()
                                .filter(|(k, _, _)| result.iter().any(|r| r == *k))
                                .map(|(_, l, _)| *l)
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        print_confirm(&mut out, prompt, &display)?;
                        return Ok(result);
                    }
                },
                KeyCode::Esc => {
                    sorry_index = None;
                }
                _ => {}
            }

            last_lines = render_features(
                &mut out, prompt, &rows, cursor, &checked, locked, sorry_index, help, last_lines,
            )?;
        }
    }
}

fn render_features(
    out: &mut io::Stdout,
    prompt: &str,
    rows: &[FeatureRow],
    cursor: usize,
    checked: &[bool],
    locked: &[&str],
    sorry_index: Option<usize>,
    help: &str,
    prev_lines: u16,
) -> io::Result<u16> {
    move_up(out, prev_lines)?;
    move_to_start_and_clear(out)?;

    // Prompt
    out.queue(style::Print(style::style("? ").green().bold()))?;
    out.queue(style::Print(style::style(prompt).bold()))?;
    out.queue(style::Print("\r\n"))?;

    let mut lines: u16 = 1; // prompt line

    for (i, row) in rows.iter().enumerate() {
        match row {
            FeatureRow::Feature { key, label, desc } => {
                if sorry_index == Some(i) {
                    out.queue(style::Print(
                        style::style("    Sorry, plyx doesn't want to break anything :(").red(),
                    ))?;
                } else {
                    let is_cursor = i == cursor;
                    let is_checked = checked.get(i).copied().unwrap_or(false);
                    let is_locked = locked.contains(key);
                    let checkbox = if is_checked || is_locked { "[x]" } else { "[ ]" };
                    let text = format!("    {checkbox} {label}: {desc}");
                    if is_cursor {
                        out.queue(style::Print(style::style(text).blue()))?;
                    } else if is_checked || is_locked {
                        out.queue(style::Print(style::style(text).green()))?;
                    } else {
                        out.queue(style::Print(text))?;
                    }
                }
            }
            FeatureRow::Action(label) => {
                let text = format!("    > {label}");
                if i == cursor {
                    out.queue(style::Print(style::style(text).blue()))?;
                } else {
                    // White / default text, not grayed out
                    out.queue(style::Print(text))?;
                }
            }
        }
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    if !help.is_empty() {
        out.queue(style::Print(style::style(format!("  {help}")).dark_grey()))?;
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    out.flush()?;
    // Cursor is `lines` rows below the start (past all content).
    Ok(lines)
}

// ── add_widget ──────────────────────────────────────────────────────────

/// Result of the combined add widget.
pub struct AddResult {
    /// Newly enabled feature keys (not including locked ones).
    pub features: Vec<String>,
    /// Newly added font names.
    pub fonts: Vec<String>,
}

/// Combined feature + font add widget for `plyx add`.
///
/// Shows features (with locked ones already checked), a font search bar,
/// search results, and a single Done! button. Arrow keys navigate between
/// features, the font search, and Done!.
///
/// `locked_features` — already-enabled feature keys (checked, green, sorry on toggle).  
/// `installed_fonts` — font names already in assets/fonts/ (green, sorry on add).
pub fn add_widget(
    prompt: &str,
    features: &[(&str, &str, &str)],
    font_items: &[String],
    locked_features: &[&str],
    installed_fonts: &[String],
    help: &str,
) -> Result<AddResult, String> {
    add_widget_inner(prompt, features, font_items, locked_features, installed_fonts, help)
        .map_err(|e| e.to_string())
}

/// Cursor can be on a feature row, the font search row, or Done!
enum AddCursorPos {
    Feature(usize),
    FontSearch,
    Done,
}

fn add_widget_inner(
    prompt: &str,
    features: &[(&str, &str, &str)],
    font_items: &[String],
    locked_features: &[&str],
    installed_fonts: &[String],
    help: &str,
) -> io::Result<AddResult> {
    let _guard = RawGuard::enter(false)?; // cursor visible for font typing
    let mut out = io::stdout();

    let mut cursor = AddCursorPos::Feature(0);
    let mut feature_checked: Vec<bool> = features
        .iter()
        .map(|(key, _, _)| locked_features.contains(key))
        .collect();
    let mut font_query = String::new();
    let mut added_fonts: Vec<String> = Vec::new();
    let mut sorry_feature: Option<usize> = None;
    let mut font_sorry = false;
    let mut last_lines: u16 = 0;

    last_lines = render_add(
        &mut out,
        prompt,
        features,
        font_items,
        locked_features,
        installed_fonts,
        &cursor,
        &feature_checked,
        &font_query,
        &added_fonts,
        sorry_feature,
        font_sorry,
        help,
        last_lines,
    )?;

    loop {
        if let Event::Key(key) = event::read()? {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                drop(_guard);
                std::process::exit(130);
            }

            match &cursor {
                AddCursorPos::Feature(idx) => {
                    let idx = *idx;
                    match key.code {
                        KeyCode::Up => {
                            if idx > 0 {
                                cursor = AddCursorPos::Feature(idx - 1);
                            }
                            sorry_feature = None;
                        }
                        KeyCode::Down => {
                            if idx + 1 < features.len() {
                                cursor = AddCursorPos::Feature(idx + 1);
                            } else {
                                cursor = AddCursorPos::FontSearch;
                            }
                            sorry_feature = None;
                        }
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            let fkey = features[idx].0;
                            if locked_features.contains(&fkey) {
                                sorry_feature = Some(idx);
                            } else {
                                if let Some(c) = feature_checked.get_mut(idx) {
                                    *c = !*c;
                                }
                                sorry_feature = None;
                            }
                        }
                        KeyCode::Esc => {
                            sorry_feature = None;
                        }
                        _ => {}
                    }
                }
                AddCursorPos::FontSearch => match key.code {
                    KeyCode::Up => {
                        if !features.is_empty() {
                            cursor = AddCursorPos::Feature(features.len() - 1);
                        }
                        font_sorry = false;
                    }
                    KeyCode::Down => {
                        cursor = AddCursorPos::Done;
                        font_sorry = false;
                    }
                    KeyCode::Enter => {
                        let filtered = filter(font_items, &font_query);
                        if let Some(top) = filtered.first() {
                            let name = (*top).clone();
                            if installed_fonts.iter().any(|f| f == &name)
                                || added_fonts.iter().any(|f| f == &name)
                            {
                                font_sorry = true;
                            } else {
                                added_fonts.push(name);
                                font_sorry = false;
                            }
                            font_query.clear();
                        }
                    }
                    KeyCode::Backspace => {
                        font_query.pop();
                        font_sorry = false;
                    }
                    KeyCode::Char(c) => {
                        font_query.push(c);
                        font_sorry = false;
                    }
                    KeyCode::Esc => {
                        font_query.clear();
                        font_sorry = false;
                    }
                    _ => {}
                },
                AddCursorPos::Done => match key.code {
                    KeyCode::Up => {
                        cursor = AddCursorPos::FontSearch;
                    }
                    KeyCode::Char(' ') | KeyCode::Enter => {
                        // Confirm
                        let new_features: Vec<String> = features
                            .iter()
                            .enumerate()
                            .filter(|(i, (key, _, _))| {
                                feature_checked.get(*i).copied().unwrap_or(false)
                                    && !locked_features.contains(key)
                            })
                            .map(|(_, (key, _, _))| key.to_string())
                            .collect();

                        move_up(&mut out, last_lines)?;
                        move_to_start_and_clear(&mut out)?;

                        let mut parts: Vec<String> = Vec::new();
                        if !new_features.is_empty() {
                            let names: Vec<&str> = new_features
                                .iter()
                                .filter_map(|k| {
                                    features
                                        .iter()
                                        .find(|(fk, _, _)| fk == k)
                                        .map(|(_, l, _)| *l)
                                })
                                .collect();
                            parts.push(format!("Features: {}", names.join(", ")));
                        }
                        if !added_fonts.is_empty() {
                            parts.push(format!("Fonts: {}", added_fonts.join(", ")));
                        }
                        let display = if parts.is_empty() {
                            "(no changes)".to_string()
                        } else {
                            parts.join(" | ")
                        };
                        print_confirm(&mut out, prompt, &display)?;

                        return Ok(AddResult {
                            features: new_features,
                            fonts: added_fonts,
                        });
                    }
                    _ => {}
                },
            }

            last_lines = render_add(
                &mut out,
                prompt,
                features,
                font_items,
                locked_features,
                installed_fonts,
                &cursor,
                &feature_checked,
                &font_query,
                &added_fonts,
                sorry_feature,
                font_sorry,
                help,
                last_lines,
            )?;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_add(
    out: &mut io::Stdout,
    prompt: &str,
    features: &[(&str, &str, &str)],
    font_items: &[String],
    locked_features: &[&str],
    installed_fonts: &[String],
    cursor: &AddCursorPos,
    feature_checked: &[bool],
    font_query: &str,
    added_fonts: &[String],
    sorry_feature: Option<usize>,
    font_sorry: bool,
    help: &str,
    prev_lines: u16,
) -> io::Result<u16> {
    // Determine if cursor ends up parked mid-widget (font search) or at the end.
    // For font search, we park the cursor at the search row. Otherwise cursor
    // ends up at the bottom.
    let park_at_font_search = matches!(cursor, AddCursorPos::FontSearch);

    move_up(out, prev_lines)?;
    move_to_start_and_clear(out)?;

    // Prompt
    out.queue(style::Print(style::style("? ").green().bold()))?;
    out.queue(style::Print(style::style(prompt).bold()))?;
    out.queue(style::Print("\r\n"))?;
    let mut lines: u16 = 1;

    // ── Feature rows
    for (i, (key, label, desc)) in features.iter().enumerate() {
        let is_cursor = matches!(cursor, AddCursorPos::Feature(ci) if *ci == i);

        if sorry_feature == Some(i) {
            out.queue(style::Print(
                style::style("    Sorry, plyx doesn't want to break anything :(").red(),
            ))?;
        } else {
            let is_checked = feature_checked.get(i).copied().unwrap_or(false);
            let is_locked = locked_features.contains(key);
            let checkbox = if is_checked || is_locked { "[x]" } else { "[ ]" };
            let text = format!("    {checkbox} {label}: {desc}");
            if is_cursor {
                out.queue(style::Print(style::style(text).blue()))?;
            } else if is_checked || is_locked {
                out.queue(style::Print(style::style(text).green()))?;
            } else {
                out.queue(style::Print(text))?;
            }
        }
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    // ── Font search row
    let font_is_cursor = matches!(cursor, AddCursorPos::FontSearch);
    out.queue(style::Print("  "))?;
    if font_is_cursor {
        out.queue(style::Print(style::style("Add fonts: ").blue()))?;
    } else {
        out.queue(style::Print("Add fonts: "))?;
    }
    if font_query.is_empty() {
        out.queue(style::Print(style::style("(type to search)").dark_grey()))?;
    } else {
        out.queue(style::Print(font_query))?;
    }
    out.queue(style::Print("\r\n"))?;
    lines += 1;
    let font_search_line = lines - 1; // 0-indexed row of font search

    // ── Font search results
    if font_sorry {
        out.queue(style::Print(
            style::style("    Sorry, plyx doesn't want to break anything :(").red(),
        ))?;
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    } else {
        let filtered = filter(font_items, font_query);
        let shown: Vec<&String> = filtered.iter().take(VISIBLE_RESULTS).copied().collect();
        for (i, item) in shown.iter().enumerate() {
            let is_installed = installed_fonts.iter().any(|f| f == *item);
            let is_added = added_fonts.iter().any(|f| f == *item);
            if i == 0 && font_is_cursor {
                out.queue(style::Print(style::style(format!("    {item}")).blue()))?;
            } else if is_installed || is_added {
                out.queue(style::Print(style::style(format!("    {item}")).green()))?;
            } else {
                out.queue(style::Print(format!("    {item}")))?;
            }
            out.queue(style::Print("\r\n"))?;
            lines += 1;
        }
    }

    // ── Done! button
    let done_is_cursor = matches!(cursor, AddCursorPos::Done);
    let done_text = "    > Done!";
    if done_is_cursor {
        out.queue(style::Print(style::style(done_text).blue()))?;
    } else {
        out.queue(style::Print(done_text))?;
    }
    out.queue(style::Print("\r\n"))?;
    lines += 1;

    // Selected summary
    let new_features: Vec<&str> = features
        .iter()
        .enumerate()
        .filter(|(i, (key, _, _))| {
            feature_checked.get(*i).copied().unwrap_or(false)
                && !locked_features.contains(key)
        })
        .map(|(_, (_, l, _))| *l)
        .collect();
    if !new_features.is_empty() || !added_fonts.is_empty() {
        let mut summary_parts = Vec::new();
        if !new_features.is_empty() {
            summary_parts.push(format!("+{}", new_features.join(", +")));
        }
        if !added_fonts.is_empty() {
            summary_parts.push(format!("+{}", added_fonts.join(", +")));
        }
        out.queue(style::Print(
            style::style(format!("  {}", summary_parts.join("  "))).dark_grey(),
        ))?;
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    if !help.is_empty() {
        out.queue(style::Print(style::style(format!("  {help}")).dark_grey()))?;
        out.queue(style::Print("\r\n"))?;
        lines += 1;
    }

    if park_at_font_search {
        // Park cursor at font search row
        move_up(out, lines - font_search_line)?;
        let col = 2 + "Add fonts: ".len() + font_query.len();
        out.queue(cursor::MoveToColumn(col as u16))?;
        out.flush()?;
        // Cursor is at font_search_line, so next re-render moves up font_search_line
        Ok(font_search_line)
    } else {
        out.flush()?;
        Ok(lines)
    }
}
