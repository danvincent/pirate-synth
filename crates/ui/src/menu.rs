pub const KEY_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub const SCALE_NAMES: [&str; 9] = [
    "N/A",
    "Major",
    "Minor",
    "Penta",
    "Dorian",
    "Mixo",
    "Whole",
    "Hirajoshi",
    "Lydian",
];

pub const BANK_NAMES: [&str; 4] = ["A", "B", "C", "D"];

/// Display names for the bytebeat algorithm options.
/// Index 0 is now "Basic" (no "Off"). "Random" is index 10.
pub const BYTEBEAT_ALGO_NAMES: [&str; 11] = ["Basic", "Sierpinski", "Melody", "Harmony", "Acid", "Wobble", "Glitch", "Pulse", "Storm", "Echo", "Random"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuContext {
    Main,
    Wavetable,
    Granular,
    Bytebeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Select,
    Back,
    Left,
    Right,
    /// Toggle wavetable oscillators on/off (GPi CASE Select button)
    ToggleWt,
    /// Toggle granular engine on/off (GPi CASE Start button)
    ToggleGranular,
    /// Step note/key up by one semitone (GPi CASE A button)
    NoteUp,
    /// Step note/key down by one semitone (GPi CASE B button)
    NoteDown,
    /// Cycle wavetable bank forward (GPi CASE X button)
    BankCycle,
    /// Cycle scale forward (GPi CASE Y button)
    ScaleCycle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoStatus {
    Off,
    On,
    NoHdmi,
}

impl VideoStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::NoHdmi => "No HDMI",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub key_index: usize,
    pub octave: i32,
    pub fine_tune_cents: f32,
    pub stereo_spread: u8,
    pub scale_index: usize,
    pub bank_index: usize,
    pub wt_volume: u8,
    pub gr_volume: u8,
    pub oscillators_active: bool,
    pub granular_active: bool,
    pub osc_count: usize,
    pub gr_voices: usize,
    pub bytebeat_algo_index: usize,
    pub video_status: VideoStatus,
    pub glide_progress: Option<f32>,
    pub selected_item: usize,
    pub scroll_offset: usize,
    pub context: MenuContext,
    pub bb_active: bool,
    pub bb_volume: u8,
    pub bb_osc_count: usize,
}

impl MenuState {
    pub fn new(fine_tune_cents: f32, osc_count: usize, gr_voices: usize) -> Self {
        Self {
            key_index: 9,
            octave: 1,
            fine_tune_cents,
            stereo_spread: 100,
            scale_index: 7,
            bank_index: 0,
            wt_volume: 50,
            gr_volume: 50,
            oscillators_active: false,
            granular_active: false,
            osc_count,
            gr_voices,
            bytebeat_algo_index: 0,
            video_status: VideoStatus::Off,
            glide_progress: None,
            selected_item: 0,
            scroll_offset: 0,
            context: MenuContext::Main,
            bb_active: false,
            bb_volume: 50,
            bb_osc_count: 4,
        }
    }

    pub fn key_name(&self) -> &'static str {
        KEY_NAMES[self.key_index]
    }

    pub fn total_items(&self) -> usize {
        match self.context {
            MenuContext::Main => 10,
            MenuContext::Wavetable => 5,
            MenuContext::Granular => 4,
            MenuContext::Bytebeat => 5,
        }
    }

    pub fn apply_button(&mut self, button: Button) {
        const VISIBLE_ROWS: usize = 11;

        // Hardware shortcut buttons always work regardless of context
        match button {
            Button::ToggleWt => {
                self.toggle_wt();
                return;
            }
            Button::ToggleGranular => {
                self.toggle_gr();
                return;
            }
            Button::NoteUp => {
                self.key_up();
                return;
            }
            Button::NoteDown => {
                self.key_down();
                return;
            }
            Button::BankCycle => {
                self.bank_next();
                return;
            }
            Button::ScaleCycle => {
                self.scale_next();
                return;
            }
            _ => {}
        }

        // Context-aware navigation and value changes
        match self.context {
            MenuContext::Main => self.handle_main_button(button),
            MenuContext::Wavetable | MenuContext::Granular | MenuContext::Bytebeat => {
                self.handle_submenu_button(button)
            }
        }

        // Adjust scroll offset to keep selected_item visible
        if self.selected_item < self.scroll_offset {
            self.scroll_offset = self.selected_item;
        } else if self.selected_item >= self.scroll_offset + VISIBLE_ROWS {
            self.scroll_offset = self.selected_item + 1 - VISIBLE_ROWS;
        }
    }

    fn handle_main_button(&mut self, button: Button) {
        match button {
            Button::Up => {
                if self.selected_item == 0 {
                    self.selected_item = self.total_items().saturating_sub(1);
                } else {
                    self.selected_item -= 1;
                }
            }
            Button::Down => {
                self.selected_item = (self.selected_item + 1) % self.total_items();
            }
            Button::Select | Button::Right => {
                // Items 0-2 are submenus
                match self.selected_item {
                    0 => {
                        self.context = MenuContext::Wavetable;
                        self.selected_item = 0;
                        self.scroll_offset = 0;
                    }
                    1 => {
                        self.context = MenuContext::Granular;
                        self.selected_item = 0;
                        self.scroll_offset = 0;
                    }
                    2 => {
                        self.context = MenuContext::Bytebeat;
                        self.selected_item = 0;
                        self.scroll_offset = 0;
                    }
                    // Items 3-9 are value adjustments
                    _ => self.increment_main_value(),
                }
            }
            Button::Back | Button::Left => self.decrement_main_value(),
            _ => {}
        }
    }

    fn handle_submenu_button(&mut self, button: Button) {
        match button {
            Button::Up => {
                if self.selected_item == 0 {
                    self.selected_item = self.total_items().saturating_sub(1);
                } else {
                    self.selected_item -= 1;
                }
            }
            Button::Down => {
                self.selected_item = (self.selected_item + 1) % self.total_items();
            }
            Button::Back | Button::Left => {
                if self.selected_item == 0 {
                    self.context = MenuContext::Main;
                    self.selected_item = 0;
                    self.scroll_offset = 0;
                } else {
                    self.decrement_submenu_value();
                }
            }
            Button::Select | Button::Right => {
                // Item 0 is always "Back"
                if self.selected_item == 0 {
                    self.context = MenuContext::Main;
                    self.selected_item = 0;
                    self.scroll_offset = 0;
                } else {
                    // Item 1+ triggers value changes
                    self.increment_submenu_value();
                }
            }
            _ => {}
        }
    }

    fn toggle_wt(&mut self) {
        self.oscillators_active = !self.oscillators_active;
    }

    fn toggle_gr(&mut self) {
        self.granular_active = !self.granular_active;
    }

    fn key_up(&mut self) {
        self.key_index = (self.key_index + 1) % KEY_NAMES.len();
    }

    fn key_down(&mut self) {
        if self.key_index == 0 {
            self.key_index = KEY_NAMES.len() - 1;
        } else {
            self.key_index -= 1;
        }
    }

    fn bank_next(&mut self) {
        self.bank_index = (self.bank_index + 1) % BANK_NAMES.len();
    }

    fn scale_next(&mut self) {
        self.scale_index = (self.scale_index + 1) % SCALE_NAMES.len();
    }

    fn increment_main_value(&mut self) {
        match self.selected_item {
            3 => self.key_up(),
            4 => self.scale_next(),
            5 => self.octave = (self.octave + 1).min(8),
            6 => self.stereo_spread = (self.stereo_spread + 5).min(100),
            7 => self.fine_tune_cents = (self.fine_tune_cents + 1.0).min(100.0),
            // 8 = Video: read-only
            // 9 = Glide: read-only
            _ => {}
        }
    }

    fn decrement_main_value(&mut self) {
        match self.selected_item {
            3 => self.key_down(),
            4 => {
                if self.scale_index == 0 {
                    self.scale_index = SCALE_NAMES.len() - 1;
                } else {
                    self.scale_index -= 1;
                }
            }
            5 => self.octave = (self.octave - 1).max(0),
            6 => self.stereo_spread = self.stereo_spread.saturating_sub(5),
            7 => self.fine_tune_cents = (self.fine_tune_cents - 1.0).max(-100.0),
            _ => {}
        }
    }

    fn increment_submenu_value(&mut self) {
        match self.context {
            MenuContext::Wavetable => match self.selected_item {
                1 => self.toggle_wt(),
                2 => self.wt_volume = (self.wt_volume + 10).min(100),
                3 => self.bank_next(),
                4 => self.osc_count = (self.osc_count + 1).min(64),
                _ => {}
            },
            MenuContext::Granular => match self.selected_item {
                1 => self.toggle_gr(),
                2 => self.gr_volume = (self.gr_volume + 10).min(100),
                3 => self.gr_voices = (self.gr_voices + 1).min(64),
                _ => {}
            },
            MenuContext::Bytebeat => match self.selected_item {
                1 => self.bb_active = !self.bb_active,
                2 => self.bb_volume = (self.bb_volume + 10).min(100),
                3 => self.bytebeat_algo_index = (self.bytebeat_algo_index + 1) % BYTEBEAT_ALGO_NAMES.len(),
                4 => self.bb_osc_count = (self.bb_osc_count + 1).min(8),
                _ => {}
            },
            MenuContext::Main => {}
        }
    }

    fn decrement_submenu_value(&mut self) {
        match self.context {
            MenuContext::Wavetable => match self.selected_item {
                1 => self.toggle_wt(),
                2 => self.wt_volume = self.wt_volume.saturating_sub(10),
                3 => self.bank_index = if self.bank_index == 0 { BANK_NAMES.len() - 1 } else { self.bank_index - 1 },
                4 => self.osc_count = self.osc_count.saturating_sub(1).max(1),
                _ => {}
            },
            MenuContext::Granular => match self.selected_item {
                1 => self.toggle_gr(),
                2 => self.gr_volume = self.gr_volume.saturating_sub(10),
                3 => self.gr_voices = self.gr_voices.saturating_sub(1).max(1),
                _ => {}
            },
            MenuContext::Bytebeat => match self.selected_item {
                1 => self.bb_active = !self.bb_active,
                2 => self.bb_volume = self.bb_volume.saturating_sub(10),
                3 => self.bytebeat_algo_index = if self.bytebeat_algo_index == 0 { BYTEBEAT_ALGO_NAMES.len() - 1 } else { self.bytebeat_algo_index - 1 },
                4 => self.bb_osc_count = self.bb_osc_count.saturating_sub(1).max(1),
                _ => {}
            },
            MenuContext::Main => {}
        }
    }

    pub fn lines(&self) -> Vec<String> {
        match self.context {
            MenuContext::Main => vec![
                "Wavetable →".to_string(),
                "Granular →".to_string(),
                "Bytebeat →".to_string(),
                format!("Key: {}", self.key_name()),
                format!("Scale: {}", SCALE_NAMES[self.scale_index]),
                format!("Octave: {}", self.octave),
                format!("Stereo: {}", self.stereo_spread),
                format!("Cents: {:+}", self.fine_tune_cents as i32),
                format!("Video: {}", self.video_status.as_str()),
                format!(
                    "Glide: {}",
                    match self.glide_progress {
                        None => "---".to_string(),
                        Some(p) => format!("{}%", (p * 100.0) as u32),
                    }
                ),
            ],
            MenuContext::Wavetable => vec![
                "Back".to_string(),
                format!("On/Off: {}", if self.oscillators_active { "On" } else { "Off" }),
                format!("Volume: {}", self.wt_volume),
                format!("Bank: {}", BANK_NAMES[self.bank_index]),
                format!("Oscillators: {}", self.osc_count),
            ],
            MenuContext::Granular => vec![
                "Back".to_string(),
                format!("On/Off: {}", if self.granular_active { "On" } else { "Off" }),
                format!("Volume: {}", self.gr_volume),
                format!("Voices: {}", self.gr_voices),
            ],
            MenuContext::Bytebeat => vec![
                "Back".to_string(),
                format!("On/Off: {}", if self.bb_active { "On" } else { "Off" }),
                format!("Volume: {}", self.bb_volume),
                format!("Algorithm: {}", BYTEBEAT_ALGO_NAMES[self.bytebeat_algo_index]),
                format!("Oscillators: {}", self.bb_osc_count),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_menu_has_ten_items() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.total_items(), 10);
        assert_eq!(menu.lines().len(), 10);
    }

    #[test]
    fn wavetable_submenu_has_five_items() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        assert_eq!(menu.total_items(), 5);
        assert_eq!(menu.lines().len(), 5);
    }

    #[test]
    fn granular_submenu_has_four_items() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Granular;
        assert_eq!(menu.total_items(), 4);
        assert_eq!(menu.lines().len(), 4);
    }

    #[test]
    fn bytebeat_submenu_has_five_items() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        assert_eq!(menu.total_items(), 5);
        assert_eq!(menu.lines().len(), 5);
    }

    #[test]
    fn main_menu_first_three_lines_are_submenus() {
        let menu = MenuState::new(0.0, 8, 8);
        let lines = menu.lines();
        assert_eq!(lines[0], "Wavetable →");
        assert_eq!(lines[1], "Granular →");
        assert_eq!(lines[2], "Bytebeat →");
    }

    #[test]
    fn select_on_wavetable_enters_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 0;
        menu.apply_button(Button::Select);
        assert_eq!(menu.context, MenuContext::Wavetable);
        assert_eq!(menu.selected_item, 0);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn select_on_granular_enters_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        assert_eq!(menu.context, MenuContext::Granular);
        assert_eq!(menu.selected_item, 0);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn select_on_bytebeat_enters_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 2;
        menu.apply_button(Button::Select);
        assert_eq!(menu.context, MenuContext::Bytebeat);
        assert_eq!(menu.selected_item, 0);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn back_exits_submenu_to_main() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        menu.selected_item = 0; // At "Back" row
        menu.apply_button(Button::Back);
        assert_eq!(menu.context, MenuContext::Main);
        assert_eq!(menu.selected_item, 0);
    }

    #[test]
    fn left_exits_submenu_to_main() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Granular;
        menu.selected_item = 0; // At "Back" row
        menu.apply_button(Button::Left);
        assert_eq!(menu.context, MenuContext::Main);
        assert_eq!(menu.selected_item, 0);
    }

    #[test]
    fn select_on_back_item_exits_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.selected_item = 0;
        menu.apply_button(Button::Select);
        assert_eq!(menu.context, MenuContext::Main);
        assert_eq!(menu.selected_item, 0);
    }

    #[test]
    fn submenu_selected_item_resets_on_entry() {
        let mut menu = MenuState::new(0.0, 8, 8);
        // Select the first submenu item (Wavetable at index 0)
        menu.selected_item = 0;
        menu.apply_button(Button::Select);
        // When entering a submenu, selected_item should reset to 0
        assert_eq!(menu.context, MenuContext::Wavetable);
        assert_eq!(menu.selected_item, 0);
    }

    #[test]
    fn main_navigation_wraps_up() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 0;
        menu.apply_button(Button::Up);
        assert_eq!(menu.selected_item, menu.total_items() - 1);
    }

    #[test]
    fn main_navigation_wraps_down() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = menu.total_items() - 1;
        menu.apply_button(Button::Down);
        assert_eq!(menu.selected_item, 0);
    }

    #[test]
    fn wt_volume_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        menu.wt_volume = 50;
        menu.selected_item = 2;
        menu.apply_button(Button::Select);
        assert_eq!(menu.wt_volume, 60);
    }

    #[test]
    fn gr_volume_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Granular;
        menu.gr_volume = 50;
        menu.selected_item = 2;
        menu.apply_button(Button::Select);
        assert_eq!(menu.gr_volume, 60);
    }

    #[test]
    fn bb_active_toggle_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.bb_active = false;
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        assert_eq!(menu.bb_active, true);
    }

    #[test]
    fn bb_volume_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.bb_volume = 50;
        menu.selected_item = 2;
        menu.apply_button(Button::Select);
        assert_eq!(menu.bb_volume, 60);
    }

    #[test]
    fn bb_algo_cycles_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.bytebeat_algo_index = 0;
        menu.selected_item = 3;
        menu.apply_button(Button::Select);
        assert_eq!(menu.bytebeat_algo_index, 1);
    }

    #[test]
    fn bb_algo_no_off_option() {
        assert_eq!(BYTEBEAT_ALGO_NAMES[0], "Basic");
    }

    #[test]
    fn bb_algo_last_is_random() {
        assert_eq!(BYTEBEAT_ALGO_NAMES[10], "Random");
    }

    #[test]
    fn bb_osc_count_increments_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.bb_osc_count = 4;
        menu.selected_item = 4;
        menu.apply_button(Button::Select);
        assert_eq!(menu.bb_osc_count, 5);
    }

    #[test]
    fn wt_oscillators_toggle_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        menu.oscillators_active = false;
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        assert_eq!(menu.oscillators_active, true);
    }

    #[test]
    fn gr_toggle_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Granular;
        menu.granular_active = false;
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        assert_eq!(menu.granular_active, true);
    }

    #[test]
    fn hardware_buttons_work_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        let initial_key = menu.key_index;
        menu.apply_button(Button::NoteUp);
        assert_eq!(menu.key_index, (initial_key + 1) % KEY_NAMES.len());
    }

    #[test]
    fn back_in_submenu_resets_selected_item() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.selected_item = 0; // At "Back" row
        menu.apply_button(Button::Back);
        assert_eq!(menu.context, MenuContext::Main);
        assert_eq!(menu.selected_item, 0);
    }

    #[test]
    fn scroll_offset_initialized_to_zero() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn glide_shows_dashes_when_none() {
        let menu = MenuState::new(0.0, 8, 8);
        let lines = menu.lines();
        assert!(lines[9].contains("Glide: ---"));
    }

    #[test]
    fn glide_shows_percent_when_some() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.glide_progress = Some(0.5);
        let lines = menu.lines();
        assert!(lines[9].contains("50%"));
    }

    #[test]
    fn glide_is_read_only_in_main() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 9;
        let before = menu.lines();
        menu.apply_button(Button::Select);
        menu.apply_button(Button::Back);
        assert_eq!(menu.lines(), before);
    }

    #[test]
    fn video_line_in_main() {
        let menu = MenuState::new(0.0, 8, 8);
        let lines = menu.lines();
        assert!(lines[8].starts_with("Video:"));
    }

    #[test]
    fn wavetable_bank_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        menu.bank_index = 0;
        menu.selected_item = 3;
        menu.apply_button(Button::Select);
        assert_eq!(menu.bank_index, 1);
    }

    #[test]
    fn wt_osc_count_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        menu.osc_count = 4;
        menu.selected_item = 4;
        menu.apply_button(Button::Select);
        assert_eq!(menu.osc_count, 5);
    }

    #[test]
    fn gr_voices_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Granular;
        menu.gr_voices = 4;
        menu.selected_item = 3;
        menu.apply_button(Button::Select);
        assert_eq!(menu.gr_voices, 5);
    }

    #[test]
    fn main_key_item_increments() {
        let mut menu = MenuState::new(0.0, 8, 8);
        let initial_key = menu.key_index;
        menu.selected_item = 3;
        menu.apply_button(Button::Select);
        assert_eq!(menu.key_index, (initial_key + 1) % KEY_NAMES.len());
    }

    #[test]
    fn default_values() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.context, MenuContext::Main);
        assert_eq!(menu.bb_active, false);
        assert_eq!(menu.bb_volume, 50);
        assert_eq!(menu.bb_osc_count, 4);
    }

    #[test]
    fn right_enters_wavetable_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 0;
        menu.apply_button(Button::Right);
        assert_eq!(menu.context, MenuContext::Wavetable);
    }

    #[test]
    fn right_enters_granular_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 1;
        menu.apply_button(Button::Right);
        assert_eq!(menu.context, MenuContext::Granular);
    }

    #[test]
    fn right_enters_bytebeat_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 2;
        menu.apply_button(Button::Right);
        assert_eq!(menu.context, MenuContext::Bytebeat);
    }

    #[test]
    fn right_increments_value_in_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        // Enter bytebeat submenu, go to volume item
        menu.selected_item = 2;
        menu.apply_button(Button::Select); // enter bytebeat
        menu.selected_item = 2; // volume item
        menu.apply_button(Button::Right);
        assert_eq!(menu.bb_volume, 60);
    }

    #[test]
    fn back_exits_without_mutating_submenu_value() {
        let mut menu = MenuState::new(0.0, 8, 8);
        // Enter bytebeat submenu
        menu.selected_item = 2;
        menu.apply_button(Button::Select);
        // Navigate to Back row (selected_item = 0)
        menu.selected_item = 0;
        let bb_active_before = menu.bb_active;
        // Press Back — should exit from Back row
        menu.apply_button(Button::Back);
        assert_eq!(menu.context, MenuContext::Main, "should be back in main");
        assert_eq!(menu.bb_active, bb_active_before, "bb_active should not have changed");
    }

    #[test]
    fn left_exits_without_mutating_submenu_value() {
        let mut menu = MenuState::new(0.0, 8, 8);
        // Enter granular submenu
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        // Navigate to Back row
        menu.selected_item = 0;
        let vol_before = menu.gr_volume;
        menu.apply_button(Button::Left);
        assert_eq!(menu.context, MenuContext::Main, "should be back in main");
        assert_eq!(menu.gr_volume, vol_before, "gr_volume should not have changed");
    }

    #[test]
    fn left_on_submenu_arrows_in_main_does_not_change_context() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.selected_item = 0; // "Wavetable →"
        menu.apply_button(Button::Left);
        assert_eq!(menu.context, MenuContext::Main);
        menu.selected_item = 1; // "Granular →"
        menu.apply_button(Button::Left);
        assert_eq!(menu.context, MenuContext::Main);
        menu.selected_item = 2; // "Bytebeat →"
        menu.apply_button(Button::Left);
        assert_eq!(menu.context, MenuContext::Main);
    }

    #[test]
    fn all_hardware_buttons_work_in_wavetable_submenu() {
        let mut menu = MenuState::new(0.0, 8, 8);
        // Enter wavetable submenu
        menu.selected_item = 0;
        menu.apply_button(Button::Select);
        assert_eq!(menu.context, MenuContext::Wavetable);

        let key_before = menu.key_index;
        menu.apply_button(Button::NoteUp);
        assert_ne!(menu.key_index, key_before, "NoteUp should work in submenu");

        let key_before = menu.key_index;
        menu.apply_button(Button::NoteDown);
        assert_ne!(menu.key_index, key_before, "NoteDown should work in submenu");

        let bank_before = menu.bank_index;
        menu.apply_button(Button::BankCycle);
        assert_ne!(menu.bank_index, bank_before, "BankCycle should work in submenu");

        let scale_before = menu.scale_index;
        menu.apply_button(Button::ScaleCycle);
        assert_ne!(menu.scale_index, scale_before, "ScaleCycle should work in submenu");

        let wt_before = menu.oscillators_active;
        menu.apply_button(Button::ToggleWt);
        assert_ne!(menu.oscillators_active, wt_before, "ToggleWt should work in submenu");

        let gr_before = menu.granular_active;
        menu.apply_button(Button::ToggleGranular);
        assert_ne!(menu.granular_active, gr_before, "ToggleGranular should work in submenu");
    }

    #[test]
    fn wt_volume_decrement_does_not_underflow_at_zero() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Wavetable;
        menu.wt_volume = 0;
        menu.selected_item = 2;
        menu.apply_button(Button::Left);
        assert_eq!(menu.wt_volume, 0, "volume should saturate at 0, not underflow");
    }

    #[test]
    fn gr_volume_decrement_does_not_underflow_at_zero() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Granular;
        menu.gr_volume = 0;
        menu.selected_item = 2;
        menu.apply_button(Button::Left);
        assert_eq!(menu.gr_volume, 0, "gr_volume should saturate at 0, not underflow");
    }

    #[test]
    fn bb_volume_decrement_does_not_underflow_at_zero() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.bb_volume = 0;
        menu.selected_item = 2;
        menu.apply_button(Button::Left);
        assert_eq!(menu.bb_volume, 0, "bb_volume should saturate at 0, not underflow");
    }

    #[test]
    fn osc_count_decrement_does_not_go_below_one() {
        let mut menu = MenuState::new(0.0, 1, 8);
        menu.context = MenuContext::Wavetable;
        menu.osc_count = 1;
        menu.selected_item = 4;
        menu.apply_button(Button::Left);
        assert_eq!(menu.osc_count, 1, "osc_count minimum is 1");
    }

    #[test]
    fn bb_osc_count_decrement_does_not_go_below_one() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.context = MenuContext::Bytebeat;
        menu.bb_osc_count = 1;
        menu.selected_item = 4;
        menu.apply_button(Button::Left);
        assert_eq!(menu.bb_osc_count, 1, "bb_osc_count minimum is 1");
    }
}
